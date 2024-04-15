// To test some of build.rs' code correctness run:
// cargo build --features=test_build_rs_of_ncurses_rs
// when doing that, the following cfg_attr ensures there are no warnings about unused stuff.
#![cfg_attr(
    all(
        feature = "test_build_rs_of_ncurses_rs",
        not(feature = "dummy_feature_to_detect_that_--all-features_arg_was_used")
    ),
    allow(dead_code)
)]
#![allow(clippy::uninlined_format_args)] // or is it more readable inlined?

extern crate cc;
extern crate pkg_config;

use pkg_config::Library;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;

// Optional environment variables:

// The below doc comment doesn't apply for these 2 env.vars:
const ENV_VAR_NAME_FOR_LIB: &str = "NCURSES_RS_RUSTC_LINK_LIB";
const ENV_VAR_NAME_FOR_NCURSES_RS_RUSTC_FLAGS: &str = "NCURSES_RS_RUSTC_FLAGS";

/// Assuming we want env.var "NCURSES_RS_CFLAGS" here,
/// and target==host and is "x86_64-unknown-linux-gnu"
/// then calls to Build::try_flags_from_environment() below in code,
/// will try the following env.vars in this order:
/// 1. "NCURSES_RS_CFLAGS_x86_64-unknown-linux-gnu" (notice dashes)
/// 2. "NCURSES_RS_CFLAGS_x86_64_unknown_linux_gnu" (notice underscores)
/// 3. "HOST_NCURSES_RS_CFLAGS" or "TARGET_NCURSES_RS_CFLAGS" (if target!=host)
/// 4. "NCURSES_RS_CFLAGS" (our original wanted)
/// and the first one that exists is used instead.
/// see: https://docs.rs/cc/1.0.92/src/cc/lib.rs.html#3571-3580
const ENV_VAR_NAME_FOR_NCURSES_RS_CFLAGS: &str = "NCURSES_RS_CFLAGS";

const IS_WIDE: bool = cfg!(all(feature = "wide", not(target_os = "macos")));

// will search for these and if not found
// then the last one in list will be used as fallback
// and still try linking with it eg. -lncursesw
const NCURSES_LIB_NAMES: &[&str] = if IS_WIDE {
    &["ncursesw5", "ncursesw"]
} else {
    &["ncurses5", "ncurses"]
};

const MENU_LIB_NAMES: &[&str] = if IS_WIDE {
    &["menuw5", "menuw"]
} else {
    &["menu5", "menu"]
};

const PANEL_LIB_NAMES: &[&str] = if IS_WIDE {
    &["panelw5", "panelw"]
} else {
    &["panel5", "panel"]
};

const TINFO_LIB_NAMES: &[&str] = if IS_WIDE {
    //elements order here matters, because:
    //Fedora has ncursesw+tinfo(without w) for wide!
    //and -ltinfow fails to link on NixOS and Fedora! so -ltinfo must be used even tho wide.
    //(presumably because tinfo doesn't depend on wideness?)
    //NixOS has only ncursesw(tinfo is presumably inside it) but -ltinfo still works for it(it's a
    //symlink to ncursesw lib)
    //Gentoo has ncursesw+tinfow
    //
    //These are tried in order and first that links is selected:
    &["tinfow5", "tinfow", "tinfo"]
} else {
    //no reason to ever fallback to tinfow here when not-wide!
    //Fedora/Gentoo has ncurses+tinfo
    //NixOS has only ncursesw(but works for non-wide), -ltinfo symlinks to ncursesw .so file)
    //so 'tinfo' is safe fallback here.
    &["tinfo5", "tinfo"]
};
//TODO: why are we trying the v5 of the lib first instead of v6 (which is the second/last in list),
//was v5 newer than the next in list? is it so on other systems?
//like: was it ever ncurses5 newer than ncurses ?
//Since we're trying v5 and it finds it, it will use it and stop looking, even though the next one
//might be v6
//This is the commit that added this v5 then v6 way: https://github.com/jeaye/ncurses-rs/commit/daddcbb557169cfac03af9667ef7aefed19f9409

/// finds and emits cargo:rustc-link-lib=
fn find_library(names: &[&str]) -> Option<Library> {
    for name in names {
        if let Ok(lib) = pkg_config::probe_library(name) {
            return Some(lib);
        }
    }
    None
}
// -----------------------------------------------------------------
// This is the normal build.rs main(),
// it's only disabled when you used: `cargo build --feature=test_build_rs_of_ncurses_rs`
#[cfg(any(
    not(feature = "test_build_rs_of_ncurses_rs"),
    feature = "dummy_feature_to_detect_that_--all-features_arg_was_used"
))]
fn main() {
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    println!(
        "cargo:rerun-if-env-changed={}",
        ENV_VAR_NAME_FOR_NCURSES_RS_RUSTC_FLAGS
    );
    println!("cargo:rerun-if-env-changed={}", ENV_VAR_NAME_FOR_LIB);

    let ncurses_lib = find_library(NCURSES_LIB_NAMES);

    if cfg!(feature = "menu") {
        if find_library(MENU_LIB_NAMES).is_none() {
            let fallback_lib_name = *MENU_LIB_NAMES.last().unwrap();
            println!("cargo:rustc-link-lib={}", fallback_lib_name);
        }
    }

    if cfg!(feature = "panel") {
        if find_library(PANEL_LIB_NAMES).is_none() {
            let fallback_lib_name = *PANEL_LIB_NAMES.last().unwrap();
            println!("cargo:rustc-link-lib={}", fallback_lib_name);
        }
    }

    //This comment block is about libtinfo.
    //If pkg-config can't find it, use fallback: 'tinfo' or 'tinfow'
    //if cargo can't find it it will ignore it gracefully - NO IT WON'T!
    //if it can find it, it will link it.
    //It's needed for ex_5 to can link  when pkg-config is missing,
    //otherwise you get this: undefined reference to symbol 'noraw'
    //Thus w/o this block, the following command would be needed to run ex_5
    //$ NCURSES_RS_RUSTC_FLAGS="-ltinfo" cargo run --features=menu --example ex_5
    //To emulate this even if you have pkg-config you can tell it to not do its job
    // by setting these env. vars before the above command:
    // $ NCURSES_NO_PKG_CONFIG=1 NCURSESW_NO_PKG_CONFIG=1 NCURSES5_NO_PKG_CONFIG=1 NCURSESW5_NO_PKG_CONFIG=1 the_rest_of_the_command_here
    // Fedora and Gentoo are two that have both ncurses(w) and tinfo(w), ie. split,
    // however Gentoo has ncurses+tinfo and ncursesw+tinfow,
    // but Fedora has ncurses+tinfo and ncursesw+tinfo (see 'tinfo' is same! no w)
    // NixOS has only ncursesw (tinfo is presumably inside?) but -lncurses -lncursesw -ltinfo work!
    // but -ltinfow doesn't work! on NixOS and Fedora!
    // On Gentoo -ltinfow works too!
    // so when pkg-config is missing, how do we know which tinfo to tell cargo to link, if any!
    // doneFIXME: ^ I guess we gonna have to compile own .c to link with tinfo to see if it fails or
    // works!
    if find_library(TINFO_LIB_NAMES).is_none() {
        //Pick the tinfo lib to link with, as fallback,
        //the first one that links successfully!
        //The order in the list matters!
        for each in TINFO_LIB_NAMES {
            if try_link(each, &ncurses_lib) {
                println!("cargo:warning=Found tinfo fallback '{}'", each);
                //successfully linked with this tinfo variant,
                //so let's use it as fallback
                println!("cargo:rustc-link-lib={}", each);
                break;
            }
        }
    }

    // gets the name of ncurses lib found by pkg-config, if it found any!
    // else (warns and)returns the default one like 'ncurses' or 'ncursesw'
    // and emits cargo:rustc-link-lib= for it unless already done.
    let lib_name = get_ncurses_lib_name(&ncurses_lib);

    if let Ok(x) = std::env::var(ENV_VAR_NAME_FOR_NCURSES_RS_RUSTC_FLAGS) {
        println!("cargo:rustc-flags={}", x);
    }

    check_chtype_size(&ncurses_lib);

    gen_rs(
        "src/genconstants.c",
        "genconstants",
        "raw_constants.rs",
        &ncurses_lib,
        &lib_name,
    );

    gen_rs(
        "src/menu/genconstants.c",
        "genmenuconstants",
        "menu_constants.rs",
        &ncurses_lib,
        &lib_name,
    );

    build_wrap(&ncurses_lib);
}
// -----------------------------------------------------------------

/// Tries to see if linker can find/link with the named library.
/// Uses ncurses lib searchdirs(if any found by pkg-config) to find that lib.
/// This is mainly used when pkg-config is missing.
/// Should still work if pkg-config exists though.
/// Returns true is linking succeeded, false otherwise.
fn try_link(lib_name: &str, ncurses_lib: &Option<Library>) -> bool {
    //OUT_DIR is set by cargo during build
    let out_dir = env::var("OUT_DIR").expect("cannot get OUT_DIR");

    //We won't execute it though, so doesn't matter if it's .exe for Windows
    let out_bin_fname = format!("try_link_with_{}", lib_name);

    //we'll generate this .c file with our contents
    let out_src_full = Path::new(&out_dir)
        .join(format!("{}.c", out_bin_fname))
        .display()
        .to_string();

    let mut file = File::create(&out_src_full).unwrap_or_else(|err| {
        panic!(
            "Couldn't create rust file '{}', reason: '{}'",
            out_src_full, err
        )
    });

    let source_code = b"int main() { return 0; }";
    file.write_all(source_code).unwrap_or_else(|err| {
        panic!(
            "Couldn't write to C file '{}', reason: '{}'",
            out_src_full, err
        )
    });
    drop(file); //explicit file close

    let build = cc::Build::new();
    let mut linker_searchdir_args: Vec<String> = Vec::new();
    //Add linker paths from ncurses lib, if any found! ie. -L
    //(this likely will be empty if pkg-config doesn't exist)
    //Include paths(for headers) don't matter! ie. -I
    if let Some(lib) = ncurses_lib {
        for link_path in &lib.link_paths {
            linker_searchdir_args.push("-L".to_string());
            linker_searchdir_args.push(link_path.display().to_string());
        }
    }

    let compiler = build
        .try_get_compiler()
        .expect("Failed Build::try_get_compiler");
    let mut command = compiler.to_command();

    let out_bin_full = Path::new(&out_dir)
        .join(out_bin_fname)
        .display()
        .to_string();
    //Create a bin(not a lib) from a .c file
    //though it wouldn't matter here if it's bin or lib, I'm
    //not sure how to find its exact output name after, to delete it.
    //Adding the relevant args for the libs that we depend upon such as ncurses
    command
        .arg("-o")
        .arg_checked(&out_bin_full)
        .arg_checked(&out_src_full)
        .args_checked(["-l", lib_name])
        .args_checked(linker_searchdir_args);
    let exit_status = command.status_or_panic(); //runs compiler
    let ret = exit_status.success();
    if !is_debug() {
        //we don't keep the generated files around, should we?
        if ret {
            //delete temporary bin that we successfully generated
            std::fs::remove_file(&out_bin_full).unwrap_or_else(|err| {
                panic!(
                    "Cannot delete generated bin file '{}', reason: '{}'",
                    out_bin_full, err
                )
            });
        }
        //delete the .c that we generated
        std::fs::remove_file(&out_src_full).unwrap_or_else(|err| {
            panic!(
                "Cannot delete generated C file '{}', reason: '{}'",
                out_src_full, err
            )
        });
    }
    return ret;
}

fn build_wrap(ncurses_lib: &Option<Library>) {
    println!("cargo:rerun-if-changed=src/wrap.c");
    let mut build = cc::Build::new();
    if let Some(lib) = ncurses_lib {
        build.includes(&lib.include_paths);
        //for path in lib.include_paths.iter() {
        //    build.include(path);
        //}
    }
    build.opt_level(1); //else is 0, causes warning on NixOS: _FORTIFY_SOURCE requires compiling with optimization (-O)

    // The following creates `libwrap.a` on linux
    build.file("src/wrap.c").compile("wrap");
}

/// Compiles an existing .c file, runs its bin to generate a .rs file from its output.
/// Uses ncurses include paths and links with ncurses lib(s)
// Note: won't link with tinfo unless pkg-config returned it.
// ie. if `pkg-config ncurses --libs` shows: -lncurses -ltinfo
// So even though we used a fallback tinfo in main, for cargo, it won't be used here. FIXME: if tinfo is needed here ever! (it's currently not, btw)
fn gen_rs(
    source_c_file: &str,
    out_bin_fname: &str,
    gen_rust_file: &str,
    ncurses_lib: &Option<Library>,
    lib_name: &str,
) {
    println!("cargo:rerun-if-changed={}", source_c_file);
    let out_dir = env::var("OUT_DIR").expect("cannot get OUT_DIR");
    #[cfg(windows)]
    let out_bin_fname = format!("{}.exe", out_bin_fname);
    let bin_full = Path::new(&out_dir)
        .join(out_bin_fname)
        .display()
        .to_string();

    //Note: env.var. "CC" can override the compiler used and will cause rebuild if changed.
    let mut build = cc::Build::new();
    let mut linker_searchdir_args: Vec<String> = Vec::new();
    if let Some(lib) = ncurses_lib {
        build.includes(&lib.include_paths);
        //for path in lib.include_paths.iter() {
        //    build.include(path);
        //}
        for link_path in &lib.link_paths {
            linker_searchdir_args.push("-L".to_string());
            linker_searchdir_args.push(link_path.display().to_string());
        }
    }

    println!(
        "cargo:rerun-if-env-changed={}",
        ENV_VAR_NAME_FOR_NCURSES_RS_CFLAGS
    );

    let _ = build.try_flags_from_environment(ENV_VAR_NAME_FOR_NCURSES_RS_CFLAGS);

    //'cc::Build' can do only lib outputs but we want a binary
    //so we get the command (and args) thus far set and add our own args.
    //Presumably all args will be kept, as per: https://docs.rs/cc/1.0.92/cc/struct.Build.html#method.get_compiler
    //(though at least the setting for build.file(source_c_file) won't be,
    // but we don't use that way and instead set it later as an arg to compiler)
    let compiler = build
        .try_get_compiler()
        .expect("Failed Build::try_get_compiler");
    let mut command = compiler.to_command();

    //create a bin(not a lib) from a .c file
    //adding the relevant args for the libs that we depend upon such as ncurses
    command
        .arg("-o")
        .arg_checked(&bin_full)
        .arg_checked(source_c_file)
        .args_checked(["-l", lib_name])
        .args_checked(linker_searchdir_args);
    command.success_or_panic(); //runs compiler

    //execute the compiled binary
    let consts = Command::new(&bin_full)
        .output()
        .unwrap_or_else(|err| panic!("Executing '{}' failed, reason: '{}'", bin_full, err));

    //write the output from executing the binary into a new rust source file .rs
    //that .rs file is later used outside of this build.rs, in the normal build
    let gen_rust_file_full_path = Path::new(&out_dir)
        .join(gen_rust_file)
        .display()
        .to_string();
    let mut file = File::create(&gen_rust_file_full_path).unwrap_or_else(|err| {
        panic!(
            "Couldn't create rust file '{}', reason: '{}'",
            gen_rust_file_full_path, err
        )
    });

    file.write_all(&consts.stdout).unwrap_or_else(|err| {
        panic!(
            "Couldn't write to rust file '{}', reason: '{}'",
            gen_rust_file_full_path, err
        )
    });
}

fn check_chtype_size(ncurses_lib: &Option<Library>) {
    let out_dir = env::var("OUT_DIR").expect("cannot get OUT_DIR");
    let src = Path::new(&out_dir)
        .join("chtype_size.c")
        .display()
        .to_string();
    let bin_name = if cfg!(windows) {
        "chtype_size.exe"
    } else {
        "chtype_size"
    };
    let bin_full = Path::new(&out_dir).join(bin_name).display().to_string();

    //TODO: do we want to keep or delete this file after ?
    let mut fp = File::create(&src)
        .unwrap_or_else(|err| panic!("cannot create '{}', reason: '{}'", src, err));
    fp.write_all(
        b"
#include <assert.h>
#include <limits.h>
#include <stdio.h>

#include <ncurses.h>

int main(void)
{
    if (sizeof(chtype)*CHAR_BIT == 64) {
        puts(\"cargo:rustc-cfg=feature=\\\"wide_chtype\\\"\");
    } else {
        /* We only support 32-bit and 64-bit chtype. */
        assert(sizeof(chtype)*CHAR_BIT == 32 && \"unsupported size for chtype\");
    }

#if defined(NCURSES_MOUSE_VERSION) && NCURSES_MOUSE_VERSION == 1
	puts(\"cargo:rustc-cfg=feature=\\\"mouse_v1\\\"\");
#endif
    return 0;
}
    ",
    )
    .unwrap_or_else(|err| panic!("cannot write into file '{}', reason: '{}'", src, err));
    drop(fp); //explicit file close (flush)

    let mut build = cc::Build::new();
    if let Some(lib) = ncurses_lib {
        build.includes(&lib.include_paths);
        //for path in lib.include_paths.iter() {
        //    build.include(path);
        //}
    }

    let _ = build.try_flags_from_environment(ENV_VAR_NAME_FOR_NCURSES_RS_CFLAGS);

    let compiler = build
        .try_get_compiler()
        .expect("Failed Build::try_get_compiler");
    let mut command = compiler.to_command();

    command.arg("-o").arg_checked(&bin_full).arg_checked(&src);
    command.success_or_panic(); //runs compiler

    let features = Command::new(&bin_full)
        .output()
        .unwrap_or_else(|err| panic!("Executing '{}' failed, reason: '{}'", bin_full, err));
    print!("{}", String::from_utf8_lossy(&features.stdout));

    //Don't delete anything we've generated, unless in --release mode or debug= is set in [profile.*]
    if !is_debug() {
        std::fs::remove_file(&src).unwrap_or_else(|err| {
            panic!("Cannot delete generated file '{}', reason: '{}'", src, err)
        });
        std::fs::remove_file(&bin_full).unwrap_or_else(|err| {
            panic!(
                "cannot delete compiled file '{}', reason: '{}'",
                bin_full, err
            )
        });
    }
}

//TODO: maybe don't delete anything we've generated? let 'cargo clean' do it.
#[inline]
fn is_debug() -> bool {
    //cargo sets DEBUG to 'true' if 'cargo build', and to 'false' if 'cargo build --release'
    //this is the -C debuginfo flag " which controls the amount of debug information included
    //in the compiled binary."
    //it actually depends on `debug=` of the profile in Cargo.toml https://doc.rust-lang.org/cargo/reference/profiles.html#debug
    //thus also doesn't need a println!("cargo:rerun-if-env-changed=DEBUG");
    // possible values here are only 'true' and 'false', even if debug="none"
    // or debug=false or debug=0 under say [profile.dev] of Cargo.toml, here
    // env.var "DEBUG" is still the string "false".
    // Also, it ignores any env.var DEBUG set before running 'cargo build'
    env::var("DEBUG").is_ok_and(|val| val != "false")
}

//call this only once, to avoid re-printing "cargo:rustc-link-lib=" // FIXME
fn get_ncurses_lib_name(ncurses_lib: &Option<Library>) -> String {
    let mut already_printed: bool = false;
    let lib_name: String = match std::env::var(ENV_VAR_NAME_FOR_LIB) {
        Ok(value) => value,
        Err(_) => {
            if let Some(ref lib) = ncurses_lib {
                // if here, `pkg-config`(shell command) via pkg_config crate,
                // has found the ncurses lib (eg. via the `ncurses.pc` file)
                // You can get something like this ["ncurses", "tinfo"] as the lib.libs vector
                // but we shouldn't assume "ncurses" is the first ie. lib.libs[0]
                // and the exact name of it can be ncurses,ncursesw,ncurses5,ncursesw5 ...
                // so find whichever it is and return that:
                let substring_to_find = "curses";
                if let Some(found) = lib.libs.iter().find(|&s| s.contains(substring_to_find)) {
                    //If we're here, the function calls to pkg_config::probe_library()
                    //from above ie. through find_library(), have already printed these:
                    //   cargo:rustc-link-lib=ncurses
                    //   cargo:rustc-link-lib=tinfo
                    //so there's no need to re-print the ncurses line as it would be the same.
                    already_printed = true;
                    found.clone()
                } else {
                    //if here, we should probably panic, but who knows it might still work even without pkg-config
                    //I've found cases where we were here and it still worked, so don't panic!

                    // Construct the repeated pkg-config command string
                    let repeated_pkg_config_command: String = NCURSES_LIB_NAMES
                        .iter()
                        .map(|ncurses_lib_name| format!("pkg-config --libs {}", ncurses_lib_name))
                        .collect::<Vec<_>>()
                        .join("` or `");

                    // Construct the warning message string with the repeated pkg-config commands
                    let warning_message = format!(
                    "pkg_config reported that it found the ncurses libs but the substring '{}' was not among them, ie. in the output of the shell command(s) eg. `{}`",
                    substring_to_find,
                    repeated_pkg_config_command
                    );

                    // Print the warning message, but use old style warning with one ":" not two "::",
                    // because old cargos(pre 23 Dec 2023) will simply ignore it and show no warning if it's "::"
                    println!("cargo:warning={}", warning_message);

                    //fallback lib name: 'ncurses' or 'ncursesw'
                    //if this fails later, there's the warning above to get an idea as to why.
                    (*NCURSES_LIB_NAMES.last().unwrap()).to_string()
                }
            } else {
                //pkg-config didn't find the lib, fallback to 'ncurses' or 'ncursesw'
                let what_lib = (*NCURSES_LIB_NAMES.last().unwrap()).to_string();
                // On FreeBSD it works without pkgconf and ncurses(6.4) installed but it will fail
                // to link ex_5 with 'menu' lib, unless `NCURSES_RS_RUSTC_FLAGS="-lmenu" is set.
                // this is why we now use fallbacks for 'menu' and 'panel` above too(not just for 'ncurses' lib)
                // that is, when pkgconf or pkg-config are missing, yet the libs are there.
                println!("cargo:warning=Using fallback lib name '{}' but if compilation fails below(like when linking ex_5 with 'menu' feature), that is why. It's likely you have not installed one of ['pkg-config' or 'pkgconf'], and/or 'ncurses' (it's package 'ncurses-devel' on Fedora). This seems to work fine on FreeBSD 14 regardless, however to not see this warning and to ensure 100% compatibility(on any OS) be sure to install, on FreeBSD, at least `pkgconf` if not both ie. `# pkg install ncurses pkgconf`.", what_lib);
                what_lib
            }
        }
    };
    if !already_printed {
        println!("cargo:rustc-link-lib={}", lib_name);
    }
    lib_name
}

// Define an extension trait for Command
trait MyCompilerCommand {
    fn success_or_panic(&mut self) -> ExitStatus;
    //fn success_or_else<F: FnOnce(ExitStatus) -> ExitStatus>(&mut self, op: F) -> ExitStatus;
    fn status_or_panic(&mut self) -> ExitStatus;
    fn show_what_will_run(&mut self) -> &mut Self;
    fn get_what_will_run(&self) -> (String, usize, String);
    fn assert_no_nul_in_args(&mut self) -> &mut Self;
    /// Panics if arg has \0 in it.
    fn args_checked<I, S>(&mut self, args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;
    /// Panics if arg has \0 aka NUL in it,
    /// otherwise the original Command::arg would've set it to "<string-with-nul>"
    /// Doesn't do any other checks, passes it to Command::arg()
    fn arg_checked<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command;
}

fn has_null_byte<S: AsRef<OsStr>>(arg: S) -> bool {
    let os_str = arg.as_ref();
    for &byte in os_str.as_bytes() {
        if byte == 0 {
            return true;
        }
    }
    false
}

/// args with \0 in them, passed to std::process::Command::arg() or ::args()
/// get replaced entirely with this: "<string-with-nul>"
const REPLACEMENT_FOR_ARG_THAT_HAS_NUL: &str = "<string-with-nul>";
// Implement the extension trait for Command
impl MyCompilerCommand for Command {
    /// you can't use an arg value "<string-with-nul>", or this will panic.
    fn success_or_panic(&mut self) -> ExitStatus {
        let exit_status: ExitStatus = self
            .show_what_will_run()
            .assert_no_nul_in_args()
            .status_or_panic();
        if exit_status.success() {
            exit_status
        } else {
            let how: String;
            if let Some(code) = exit_status.code() {
                how = format!(" with exit code {}", code);
            } else {
                how = ", was terminated by a signal".to_string();
            }
            panic!(
                "Compiler failed{}. Is ncurses installed? \
        pkg-config or pkgconf too? \
        it's 'ncurses-devel' on Fedora; \
        run `nix-shell` first, on NixOS. \
        Or maybe it failed for different reasons which are seen in the errored output above.",
                how
            )
        }
    }
    //note: can't override arg/args because they're not part of a Trait in Command
    //so would've to wrap Command in my own struct for that. This would've ensured
    //that any added args were auto-checked.
    fn args_checked<I, S>(&mut self, args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg_checked(arg.as_ref());
        }
        self
    }
    fn arg_checked<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        if has_null_byte(&arg) {
            //If the arg has NUL ie. \0  in it then arg got replaced already
            //with "<string-with-nul>", internally, by std::process::Command::arg() .
            //The found arg here will be shown with \0 in this Debug way.
            panic!(
                "Found arg '{:?}' that has at least one \\0 aka nul in it! \
                   This would've been replaced with '{}'.",
                arg.as_ref(),
                REPLACEMENT_FOR_ARG_THAT_HAS_NUL
            );
        }
        self.arg(arg)
    }
    /// Beware if user set the arg on purpose to the value of REPLACEMENT_FOR_ARG_THAT_HAS_NUL
    /// which is "<string-with-nul>" then this will panic, it's a false positive.
    fn assert_no_nul_in_args(&mut self) -> &mut Self {
        let args = self.get_args();
        for (count, arg) in args.enumerate() {
            if let Some(fully_utf8_arg) = arg.to_str() {
                //If the arg had NUL ie. \0  in it then arg got replaced already
                //with "<string-with-nul>", internally, by std::process::Command::arg() .
                if fully_utf8_arg == REPLACEMENT_FOR_ARG_THAT_HAS_NUL {
                    panic!(
                        "Found arg number '{}' that has \\0 aka NUL in it! \
                           It got replaced with '{}'.",
                        count + 1,
                        REPLACEMENT_FOR_ARG_THAT_HAS_NUL
                    );
                }
            }
        }
        self
    }
    fn get_what_will_run(&self) -> (String, usize, String) {
        let program = self.get_program();
        let p_prog = program
            .to_str()
            .unwrap_or_else(|| panic!("Compiler executable {:?} isn't valid rust string", program));
        let args = self.get_args();
        let how_many_args: usize = args.len();
        let formatted_args: String = args
            .map(|arg| {
                //If the arg had NUL ie. \0  in it then arg got replaced already
                //with "<string-with-nul>", internally, by std::process::Command::arg()
                //if it was added via Command::arg() or Command::args().
                //To prevent that use Command::arg_checked() and ::args_checked()
                if let Some(fully_utf8_arg) = arg.to_str() {
                    fully_utf8_arg.to_string()
                } else {
                    //None aka not fully utf8 arg
                    //then we show it as ascii + hex
                    let mut broken_arg = String::new();
                    use std::fmt::Write; // can't globally import this ^, conflicts with std::io::Write
                    for byte in arg.as_bytes() {
                        match std::char::from_u32(*byte as u32) {
                            Some(c) if c.is_ascii() => broken_arg.push(c),
                            _ => {
                                write!(&mut broken_arg, "\\x{:02X}", byte).expect("Failed to write")
                            }
                        }
                    }
                    broken_arg
                }
            })
            .collect::<Vec<String>>()
            .join("\" \"");
        //TODO: maybe a better way to get the args as a Vec<String> and impl Display ? but not
        //for the generic Vec<String> i think. Then, we won't have to return how_many_args!

        //return this tuple
        (
            p_prog.to_string(),
            how_many_args,
            format!("\"{}\"", formatted_args),
        )
    }
    /// just like Command::status() but panics if it can't execute it,
    /// ie. if status() would've returned an Err
    /// returns ExitStatus whether it be 0 or !=0
    fn status_or_panic(&mut self) -> ExitStatus {
        // Call the original status() method and handle the potential error
        self.status().unwrap_or_else(|err| {
            let (p_prog, how_many_args, formatted_args) = self.get_what_will_run();
            panic!(
                "Failed to run compilation command '{}' with '{}' args: '{}', reason: '{}'",
                p_prog, how_many_args, formatted_args, err
            )
        })
    }
    fn show_what_will_run(&mut self) -> &mut Self {
        let (exe_name, how_many_args, formatted_args) = self.get_what_will_run();
        eprintln!(
            "Next, attempting to run compilation command '{}' with '{}' args: '{}'",
            exe_name, how_many_args, formatted_args
        );
        self
    }
}

/// This is used to test build.rs, run with: cargo build --features=test_build_rs_of_ncurses_rs
/// This won't happen if you use --all-features
#[cfg(all(
    feature = "test_build_rs_of_ncurses_rs",
    not(feature = "dummy_feature_to_detect_that_--all-features_arg_was_used")
))]
fn main() {
    test_assert_works();
    test_invalid_utf8_in_program();
    test_nul_in_arg_unchecked();
    test_nul_in_arg();
    test_no_panic_in_command();
    test_panic_for_not_found_command();
    test_panic_for_command_non_zero_exit();
    test_get_what_will_run();
    test_assert_no_nul_in_args();

    eprintln!("\n-------------------------------------
              \n!!! All build.rs tests have passed successfully! Ignore the above seemingly erroneous output, it was part of the successful testing !!!\nYou're seeing this because you tried to build with --features=test_build_rs_of_ncurses_rs");

    // This stops the build from continuing which will fail in other places due to build.rs not
    // doing its job, since we've only just tested build.rs not used it to generate stuff.
    std::process::exit(5);
}
//The test functions are left outside of 'test_build_rs_of_ncurses_rs' feature gate
//so that they're tested to still compile ok.

#[allow(dead_code)]
fn test_assert_works() {
    let result = std::panic::catch_unwind(|| {
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(false, "!! just tested if asserts are enabled !!");
        }
    });
    #[allow(clippy::manual_assert)]
    if result.is_ok() {
        panic!("Assertions are disabled in build.rs, should not happen!");
    }
}

#[allow(dead_code)]
fn test_no_panic_in_command() {
    let expected_ec = 42;
    let cmd = if cfg!(windows) { "cmd" } else { "sh" };
    let args_ok = &["-c", "exit 0"];
    let args_fail = &["-c", &format!("exit {}", expected_ec)];
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(cmd);
        command.args(args_ok);
        //execute: sh -c 'exit 0'`
        command.status_or_panic();
    });
    let fail_msg = format!(
        "!!! This should not have panicked! Unless you don't have '{}' command, in PATH={:?} !!!",
        cmd,
        std::env::var("PATH")
    );
    assert!(result.is_ok(), "{}", fail_msg);

    // executed bin exits with exit code 0, or it would panic ie. fail the test
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(cmd);
        command.args(args_ok);
        //execute: sh -c 'exit 0'`
        command.success_or_panic();
    });
    assert!(result.is_ok(), "{}", fail_msg);

    // executed bin exits with specific exit code 2
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(cmd);
        command.args(args_fail);
        //execute: sh -c 'exit 42'`
        let exit_status = command.status_or_panic();
        assert_eq!(
            exit_status.code().expect("was command killed by a signal?"),
            expected_ec,
            "Command should've exited with exit code '{}'.",
            expected_ec
        );
    });
    assert!(result.is_ok(), "{}", fail_msg);
}

#[allow(dead_code)]
fn test_panic_for_not_found_command() {
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new("some non-exitent command");
        command.args([OsString::from("ar♥g1")]);
        command.status_or_panic();
    });
    let expected_panic_msg=
     "Failed to run compilation command 'some non-exitent command' with '1' args: '\"ar♥g1\"', reason: 'No such file or directory (os error 2)'";
    expect_panic(result, expected_panic_msg);

    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new("some non-exitent command");
        command.args([OsString::from("ar♥g1")]);
        command.success_or_panic();
    });
    expect_panic(result, expected_panic_msg);
}

#[allow(dead_code)]
fn test_panic_for_command_non_zero_exit() {
    let cmd = if cfg!(windows) { "cmd" } else { "sh" };
    let args_fail = &["-c", &format!("exit 43")];
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(cmd);
        command.args(args_fail);
        command.success_or_panic();
    });
    let expected_panic_msg = "Compiler failed with exit code 43. Is ncurses installed? pkg-config or pkgconf too? it's 'ncurses-devel' on Fedora; run `nix-shell` first, on NixOS. Or maybe it failed for different reasons which are seen in the errored output above.";
    expect_panic(result, expected_panic_msg);
}

#[allow(dead_code)]
fn test_invalid_utf8_in_program() {
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(OsString::from_vec(
            b"test_invalid_utf8_\xFFin_program".to_vec(),
        ));
        command.args([
            OsString::from("ar♥g1"),
            OsString::from_vec(b"my\xffarg3".to_vec()),
        ]);
        command.status_or_panic();
    });
    expect_panic(
        result,
        "Compiler executable \"test_invalid_utf8_\\xFFin_program\" isn't valid rust string",
    );
}

fn expect_panic(result: Result<(), Box<dyn std::any::Any + Send>>, expected_panic_message: &str) {
    if result.is_err() {
        if let Some(err) = result.unwrap_err().downcast_ref::<String>() {
            // Uncomment this to can copy/paste it for asserts:
            //println!("!!!!!!!!!! Panic message: {:?}", err);
            assert_eq!(
                err, expected_panic_message,
                "!!! Got different panic message than expected !!!"
            );
        }
    } else {
        panic!(
            "No panic was thrown! But was expecting this panic: '{}'",
            expected_panic_message
        );
    };
}

#[allow(dead_code)]
fn test_nul_in_arg_unchecked() {
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new("test_nul_in_arg_unchecked.exe");
        command.args([
            OsString::from("ar♥g1"),
            OsString::from("a\0rg2"),
            OsString::from_vec(b"my\xffarg3".to_vec()),
        ]);
        command.status_or_panic();
    });
    expect_panic(result,
         "Failed to run compilation command 'test_nul_in_arg_unchecked.exe' with '3' args: '\"ar♥g1\" \"<string-with-nul>\" \"my\\xFFarg3\"', reason: 'nul byte found in provided data'"
        );
}

#[allow(dead_code)]
fn test_nul_in_arg() {
    //via .arg()
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new("test_nul_in_arg.exe");
        command.arg_checked(OsString::from("ar♥g1"));
        command.arg_checked(
            // would panic here
            OsString::from("a\0rg2"),
        );
        command.arg_checked(OsString::from_vec(b"my\xffarg3".to_vec()));
        command.status_or_panic();
    });
    let expected_panic_msg=
         "Found arg '\"a\\0rg2\"' that has at least one \\0 aka nul in it! This would've been replaced with '<string-with-nul>'.";
    expect_panic(result, expected_panic_msg);
    //via .args()
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new("test_nul_in_args.exe");
        command.args_checked([
            // would panic here
            OsString::from("ar♥g1"),
            OsString::from("a\0rg2"),
            OsString::from_vec(b"my\xffarg3".to_vec()),
        ]);
        command.status_or_panic();
    });
    expect_panic(result, expected_panic_msg);
}

#[allow(dead_code)]
fn test_get_what_will_run() {
    let expected_prog = "test_get_what_will_run.exe";
    let mut command = Command::new(expected_prog);
    command.arg_checked(OsString::from("ar♥g1"));
    command.args_checked([
        // would panic here
        OsString::from_vec(b"my\xffarg3".to_vec()),
        OsString::from("arg4"),
    ]);
    command.arg_checked(OsString::from_vec(b"my\xffarg3".to_vec()));
    let (prog, how_many_args, formatted_args) = command.get_what_will_run();
    let expected_hma = 4;
    let expected_fa = "\"ar♥g1\" \"my\\xFFarg3\" \"arg4\" \"my\\xFFarg3\"";
    assert_eq!(prog, expected_prog);
    assert_eq!(how_many_args, expected_hma);
    assert_eq!(formatted_args, expected_fa);
}

#[allow(dead_code)]
fn test_assert_no_nul_in_args() {
    let expected_prog = "test_get_what_will_run.exe";
    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(expected_prog);
        command.arg("a\0here");
        command.assert_no_nul_in_args();
    });
    expect_panic(
        result,
        r##"Found arg number '1' that has \0 aka NUL in it! It got replaced with '<string-with-nul>'."##,
    );

    let result = std::panic::catch_unwind(|| {
        let mut command = Command::new(expected_prog);
        command.arg("no nul in this arg here");
        command.assert_no_nul_in_args();
    });
    assert!(result.is_ok(), "!!! This should not have panicked !!!");
}
