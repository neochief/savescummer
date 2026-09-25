//! Windows 10/11 x64 (PLAN-BUILD.md Windows): a per-user Inno Setup
//! installer wrapping `build/<mode>/package/SaveScummer-windows-x64/`.

use std::ffi::OsString;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

use crate::naming::{self, CLI, HOST, UI};
use crate::package::{self, Inputs, Layout};
use crate::paths::{self, Mode};
use crate::{cmd, pins};

pub const PLATFORM: naming::Platform = naming::WINDOWS;
pub const RUST_TARGET: &str = "x86_64-pc-windows-msvc";
/// `aqt install-qt <host> desktop <version> <arch>` and the folder it creates.
pub const QT_AQT_HOST: &str = "windows";
pub const QT_AQT_ARCH: &str = "win64_msvc2022_64";
pub const QT_KIT_DIR: &str = "msvc2022_64";

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn package_name() -> String {
    PLATFORM.stem()
}

/// Windows builds need nothing beyond the toolchains.
pub fn check_build_machine() -> anyhow::Result<()> {
    Ok(())
}

pub fn cmake_args() -> Vec<String> {
    Vec::new()
}

/// Lets freshly built Qt programs find the kit's DLLs.
pub fn qt_runtime_env(command: &mut Command, kit: &Path) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![kit.join("bin")];
    dirs.extend(std::env::split_paths(&path));
    command.env("PATH", std::env::join_paths(dirs).expect("PATH entries are valid"));
}

/// Where the program with the fixed executable name `name` is in a package.
pub fn program(package: &Path, name: &str) -> PathBuf {
    package.join("bin").join(naming::exe(name))
}

/// Keeps console programs from flashing a window.
pub fn hide_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Starts `exe` so it outlives xtask: no console, its own Ctrl+C group, and
/// stdout/stderr going to `log`.
///
/// It inherits exactly its stdio handles (NUL and the log). A plain spawn would pass
/// on every inheritable handle xtask holds, including the pipes xtask's own
/// output goes to (handed down by cargo and the shell), and whoever reads that
/// output (a terminal pipeline, a VS Code task, CI) would wait until the host
/// exits. std can't restrict inheritance yet, hence CreateProcessW.
pub fn spawn_detached(exe: &Path, args: &[OsString], log: &fs::File) -> anyhow::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
        UpdateProcThreadAttribute,
    };

    let null = fs::File::open("NUL").context("opening NUL")?;
    let handles: [HANDLE; 2] = [null.as_raw_handle() as HANDLE, log.as_raw_handle() as HANDLE];
    let mut line: Vec<u16> = std::iter::once(exe.as_os_str())
        .chain(args.iter().map(OsString::as_os_str))
        .map(quote_arg)
        .collect::<Vec<_>>()
        .join(&(' ' as u16));
    line.push(0);
    let application: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();

    // SAFETY: every pointer passed below points at a live local that outlives
    // the call; the attribute list buffer is sized by the first call and
    // deleted before it's freed; the returned handles are closed.
    unsafe {
        for handle in handles {
            if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                return Err(std::io::Error::last_os_error()).context("preparing the host's stdio");
            }
        }
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
        let mut buffer = vec![0u8; size];
        let list = buffer.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
            return Err(std::io::Error::last_os_error()).context("InitializeProcThreadAttributeList");
        }
        let updated = UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            std::mem::size_of_val(&handles),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if updated == 0 {
            let error = std::io::Error::last_os_error();
            DeleteProcThreadAttributeList(list);
            return Err(error).context("UpdateProcThreadAttribute");
        }

        let mut info: STARTUPINFOEXW = std::mem::zeroed();
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.StartupInfo.hStdInput = handles[0];
        info.StartupInfo.hStdOutput = handles[1];
        info.StartupInfo.hStdError = handles[1];
        info.lpAttributeList = list;
        let mut process: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null(),
            std::ptr::null(),
            &info.StartupInfo,
            &mut process,
        );
        let error = std::io::Error::last_os_error();
        DeleteProcThreadAttributeList(list);
        if created == 0 {
            return Err(error).with_context(|| format!("starting {}", exe.display()));
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
        Ok(process.dwProcessId)
    }
}

/// One argument, quoted the way the MSVC runtime parses command lines.
fn quote_arg(arg: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = arg.encode_wide().collect();
    let needs = wide.is_empty() || wide.iter().any(|&c| c == ' ' as u16 || c == '\t' as u16 || c == '"' as u16);
    if !needs {
        return wide;
    }
    const BACKSLASH: u16 = 0x5C;
    let mut out = vec!['"' as u16];
    let mut backslashes = 0;
    for c in wide {
        if c == BACKSLASH {
            backslashes += 1;
        } else {
            // Backslashes before a quote are doubled, and the quote escaped.
            if c == '"' as u16 {
                out.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(c);
    }
    // Trailing backslashes are doubled so the closing quote stays a quote.
    out.extend(std::iter::repeat_n(BACKSLASH, backslashes));
    out.push('"' as u16);
    out
}

/// Asks a windowed program to close, like clicking its close button.
pub fn ask_to_close(pid: u32) {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    hide_window(&mut command);
    let _ = command.status();
}

/// ```text
/// bin\SaveScummer.exe        host: the app, what the Start menu runs
/// bin\SaveScummer.UI.exe     UI, with Qt DLLs (once it exists)
/// bin\SaveScummer.CLI.exe    CLI
/// bin\vcruntime140.dll …     Visual C++ runtime, so no redistributable is needed
/// README.txt
/// ```
pub fn fill_package(root: &Path, inputs: &Inputs) -> anyhow::Result<Layout> {
    let bin = root.join("bin");
    let mut required = vec![PathBuf::from("bin").join(naming::exe(HOST)), PathBuf::from("bin").join(naming::exe(CLI))];

    if let Some(ui) = inputs.ui {
        package::copy_dir(&ui.install, root)?;
        required.push(PathBuf::from("bin").join(naming::exe(UI)));
    }
    package::copy_file(&inputs.host, &bin.join(naming::exe(HOST)))?;
    package::copy_file(&inputs.cli, &bin.join(naming::exe(CLI)))?;

    let runtime = vc_runtime()?;
    for dll in fs::read_dir(&runtime).with_context(|| format!("reading {}", runtime.display()))? {
        let dll = dll?.path();
        if dll.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")) {
            package::copy_file(&dll, &bin.join(dll.file_name().expect("a file")))?;
        }
    }
    required.push(PathBuf::from("bin").join("vcruntime140.dll"));

    match inputs.mode {
        Mode::Dev => {
            for exe in [&inputs.host, &inputs.cli] {
                let pdb =
                    exe.with_file_name(exe.file_stem().expect("a file").to_string_lossy().replace('-', "_") + ".pdb");
                if pdb.is_file() {
                    // Keeps Cargo's name: the executable refers to it by that name.
                    package::copy_file(&pdb, &bin.join(pdb.file_name().expect("a file")))?;
                }
            }
        }
        Mode::Release => {
            for file in package::files(root)? {
                if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdb")) {
                    fs::remove_file(&file)?;
                }
            }
        }
    }

    fs::write(root.join("README.txt"), readme(inputs)).context("writing README.txt")?;
    required.push(PathBuf::from("README.txt"));
    Ok(Layout { resources: root.to_path_buf(), required, unsummed: Vec::new() })
}

pub fn finish_package(_root: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn readme(inputs: &Inputs) -> String {
    let template = paths::packaging().join("windows").join("README.txt");
    let qt = paths::packaging().join("windows").join("README-qt.txt");
    let mut text = fs::read_to_string(&template).unwrap_or_default();
    if inputs.ui.is_some() {
        text.push('\n');
        text.push_str(&fs::read_to_string(&qt).unwrap_or_default());
    }
    let ui =
        if inputs.ui.is_some() { "  SaveScummer.UI.exe     the main window, started by SaveScummer.exe\n" } else { "" };
    text.replace("{ui}", ui)
        .replace("{version}", inputs.version)
        .replace("{qt_version}", pins::QT_VERSION)
        .replace('\n', "\r\n")
}

/// The Visual C++ runtime DLLs of the newest Visual Studio (found with vswhere).
fn vc_runtime() -> anyhow::Result<PathBuf> {
    let install = visual_studio()?;
    let redist = install.join("VC").join("Redist").join("MSVC");
    let newest = fs::read_dir(&redist)
        .with_context(|| {
            format!(
                "no Visual C++ redistributables in {} — install the \"Ui development with C++\" workload",
                redist.display()
            )
        })?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let version: Option<Vec<u32>> = name.split('.').map(|p| p.parse().ok()).collect();
            Some((version?, e.path()))
        })
        .max()
        .map(|(_, path)| path.join("x64"))
        .with_context(|| format!("no Visual C++ redistributables in {}", redist.display()))?;
    fs::read_dir(&newest)?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            name.starts_with("Microsoft.VC") && name.ends_with(".CRT")
        })
        .with_context(|| format!("no Visual C++ runtime in {}", newest.display()))
}

fn visual_studio() -> anyhow::Result<PathBuf> {
    let program_files =
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).unwrap_or(PathBuf::from(r"C:\Program Files (x86)"));
    let vswhere = program_files.join("Microsoft Visual Studio").join("Installer").join("vswhere.exe");
    if !vswhere.is_file() {
        bail!(
            "Visual Studio not found — install Visual Studio 2022+ (or its Build Tools) with \"Ui development with C++\""
        );
    }
    let path = cmd::output(Command::new(&vswhere).args([
        "-latest",
        "-products",
        "*",
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-property",
        "installationPath",
    ]))?;
    if path.is_empty() {
        bail!(
            "no Visual Studio with the C++ tools found — add \"Ui development with C++\" in the Visual Studio Installer"
        );
    }
    Ok(PathBuf::from(path))
}

fn iscc() -> PathBuf {
    paths::tools().join("inno-setup").join("ISCC.exe")
}

/// Compiles the installer from the release package into `dist/`.
pub fn release_file(package: &Path, version: &str) -> anyhow::Result<PathBuf> {
    let iscc = cmd::tool(&iscc(), &format!("Inno Setup {}", pins::INNO_VERSION), "inno")?;
    let dist = paths::dist();
    let name = PLATFORM.release_file(version);
    let mut command = Command::new(iscc);
    command
        .arg("/Q")
        .arg(format!("/DAppVersion={version}"))
        .arg(format!("/DPayload={}", package.display()))
        .arg(format!("/DOutputDir={}", dist.display()))
        .arg(format!("/DOutputBaseFilename={}", name.trim_end_matches(".exe")))
        .arg(format!("/DRepoRoot={}", paths::root().display()))
        .arg(format!("/DMinWindows={}", pins::MIN_WINDOWS))
        .arg(paths::packaging().join("windows").join("savescummer.iss"));
    cmd::run(&mut command)?;
    Ok(dist.join(name))
}

/// `setup inno`: the pinned Inno Setup, checked by SHA-256, installed
/// silently in portable mode into `.runtime/tools/inno-setup/`.
pub fn setup_inno() -> anyhow::Result<()> {
    if iscc().is_file() {
        println!("Inno Setup {} is already installed in {}", pins::INNO_VERSION, paths::show(&iscc()));
        return Ok(());
    }
    let installer = crate::setup::download(pins::INNO_URL, pins::INNO_SHA256, "innosetup.exe")?;
    let dir = paths::tools().join("inno-setup");
    let mut command = Command::new(&installer);
    command
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/CURRENTUSER", "/NOICONS", "/PORTABLE=1"])
        .arg(format!("/DIR={}", dir.display()));
    cmd::run(&mut command)?;
    let _ = fs::remove_file(&installer);
    if !iscc().is_file() {
        bail!("the Inno Setup installer finished but {} is missing", paths::show(&iscc()));
    }
    println!("installed Inno Setup {} into {}", pins::INNO_VERSION, paths::show(&dir));
    Ok(())
}

pub fn setup_linux_tools() -> anyhow::Result<()> {
    bail!("`setup linux-tools` is only for Linux builds")
}
