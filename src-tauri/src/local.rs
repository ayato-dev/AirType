//! A small model on this computer: llama.cpp's server with Qwen3.5, both
//! downloaded from inside the app and checked against known hashes. No key, and
//! no internet once they are here.
//!
//! The server starts on the first request and quits after ten idle minutes, so
//! the gigabyte or two it takes is only held while the user actually works.

use crate::state;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar, Mutex, Once,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is good for, in the user's words.
    pub note: &'static str,
    file: &'static str,
    url: &'static str,
    pub size: u64,
    sha256: &'static str,
}

/// Qwen3.5 knows 201 languages and is Apache 2.0. Q4_K_M keeps nearly all of its
/// quality at a quarter of the size.
pub const MODELS: &[Model] = &[
    Model {
        id: "qwen3.5-2b",
        name: "Qwen3.5 2B",
        note: "Быстрая, для перевода и исправлений",
        file: "Qwen3.5-2B-Q4_K_M.gguf",
        url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf",
        size: 1_280_835_840,
        sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
    },
    Model {
        id: "qwen3.5-4b",
        name: "Qwen3.5 4B",
        note: "Точнее и пишет живее, но вдвое медленнее",
        file: "Qwen3.5-4B-Q4_K_M.gguf",
        url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf",
        size: 2_740_937_888,
        sha256: "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4",
    },
];

/// The llama.cpp build the server comes from. A pinned build, so the hashes hold.
const LLAMA_BUILD: &str = "b11669";

struct Runtime {
    asset: &'static str,
    size: u64,
    sha256: &'static str,
}

/// The build for this system. On x64 Windows and Linux it is the Vulkan one,
/// which runs on any NVIDIA, AMD or Intel GPU with its usual driver and falls
/// back to the CPU without one; Apple silicon has Metal; the rest is CPU only.
fn runtime() -> Option<Runtime> {
    let (asset, size, sha256) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => (
            "llama-b11669-bin-win-vulkan-x64.zip",
            32_182_350,
            "2c07574e7bde71321cb785282b20cc821e951132149eeceeea32ddebdbf0598e",
        ),
        ("windows", "aarch64") => (
            "llama-b11669-bin-win-cpu-arm64.zip",
            12_442_177,
            "9cff39ffed17edcd03a7d876073f74d7b48cd43a1e5eafdd59ee094185ad871b",
        ),
        ("macos", "aarch64") => (
            "llama-b11669-bin-macos-arm64.tar.gz",
            12_191_582,
            "dbed55493d72bc1ba2babc8770d669c2289b5bc4ee9fb2a4dfc158d45b7d13fd",
        ),
        ("macos", "x86_64") => (
            "llama-b11669-bin-macos-x64.tar.gz",
            11_677_404,
            "cab28238d78158109b48deb27b05d50e0dcef56275d2e76da9ca8281526c7532",
        ),
        ("linux", "x86_64") => (
            "llama-b11669-bin-ubuntu-vulkan-x64.tar.gz",
            30_497_889,
            "e8709e5ba6c1b093dea7f3de57f4aba842f9a188fba6b465782d65d6fcfa4d9f",
        ),
        ("linux", "aarch64") => (
            "llama-b11669-bin-ubuntu-arm64.tar.gz",
            13_900_160,
            "9ac6fda2058d35677608a06f751a50122ddfa4ce0ddb7f084466b8e79b6d4a99",
        ),
        _ => return None,
    };
    Some(Runtime { asset, size, sha256 })
}

pub fn supported() -> bool {
    runtime().is_some()
}

pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map(|dir| dir.join("local")).map_err(|e| e.to_string())
}

fn model_path(app: &AppHandle, model: &Model) -> Result<PathBuf, String> {
    Ok(root(app)?.join("models").join(model.file))
}

/// One folder per build, named after its archive, so a switch to another
/// build (CPU to Vulkan) downloads it instead of using the old one.
fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let runtime = runtime().ok_or("Для этой системы нет сборки llama.cpp")?;
    let name = runtime.asset.trim_end_matches(".zip").trim_end_matches(".tar.gz");
    Ok(root(app)?.join(name))
}

const SERVER_EXE: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };

/// The server inside the unpacked build, wherever the archive put it.
fn find_server(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // A folder that can't be read just has no server in it.
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|name| name == SERVER_EXE) {
                return Some(path);
            }
        }
    }
    None
}

/// The model and the server that runs it are both here.
pub fn downloaded(app: &AppHandle, model: &Model) -> bool {
    model_file_ready(app, model) && runtime_ready(app)
}

fn model_file_ready(app: &AppHandle, model: &Model) -> bool {
    model_path(app, model).is_ok_and(|path| fs::metadata(path).is_ok_and(|m| m.len() == model.size))
}

fn runtime_ready(app: &AppHandle) -> bool {
    runtime_dir(app).is_ok_and(|dir| find_server(&dir).is_some())
}

// ---------- download ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub model: String,
    /// `server`, `model`, `check`, `done`, `error` or `cancelled`.
    pub stage: &'static str,
    pub done: u64,
    pub total: u64,
    pub message: Option<String>,
}

static DOWNLOAD: Mutex<Option<Progress>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn progress() -> Option<Progress> {
    DOWNLOAD.lock().unwrap().clone()
}

fn report(app: &AppHandle, progress: Progress) {
    let finished = matches!(progress.stage, "done" | "error" | "cancelled");
    let _ = app.emit_to("main", "local-progress", &progress);
    *DOWNLOAD.lock().unwrap() = (!finished).then_some(progress);
}

/// Fetches the server (once) and the model in the background, telling the
/// settings window how it goes. The model becomes the chosen one when it is in.
pub fn download(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = model(id).ok_or("Нет такой модели")?;
    let runtime = runtime().ok_or("Для этой системы локальной модели пока нет")?;
    {
        let mut current = DOWNLOAD.lock().unwrap();
        if current.is_some() {
            return Err("Уже качаю, подожди".into());
        }
        *current =
            Some(Progress { model: model.id.into(), stage: "server", done: 0, total: 0, message: None });
    }
    CANCEL.store(false, Ordering::SeqCst);
    let app = app.clone();
    thread::spawn(move || {
        let result = fetch_all(&app, model, &runtime);
        let (stage, message) = match result {
            Ok(()) => ("done", None),
            Err(_) if CANCEL.load(Ordering::SeqCst) => ("cancelled", None),
            Err(e) => ("error", Some(e)),
        };
        if stage == "done" {
            let _ = state::update_settings(&app, |settings| settings.local.model = model.id.into());
        }
        report(&app, Progress { model: model.id.into(), stage, done: 0, total: 0, message });
    });
    Ok(())
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn fetch_all(app: &AppHandle, model: &'static Model, runtime: &Runtime) -> Result<(), String> {
    let root = root(app)?;
    fs::create_dir_all(root.join("models")).map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    if !runtime_ready(app) {
        let url = format!(
            "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_BUILD}/{}",
            runtime.asset
        );
        let archive = root.join(format!("{}.part", runtime.asset));
        fetch(app, &client, &url, &archive, runtime.size, runtime.sha256, model.id, "server")?;
        let dir = runtime_dir(app)?;
        unpack(&archive, &dir, runtime.asset)?;
        let _ = fs::remove_file(&archive);
        remove_old_runtimes(&root, &dir);
    }

    if !model_file_ready(app, model) {
        let path = model_path(app, model)?;
        let part = path.with_extension("gguf.part");
        fetch(app, &client, model.url, &part, model.size, model.sha256, model.id, "model")?;
        fs::rename(&part, &path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Builds from earlier versions of the app take tens of megabytes for nothing.
fn remove_old_runtimes(root: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path != keep && path.is_dir() && name.to_string_lossy().starts_with("llama-") {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Downloads to `path`, picking up a cut-off download where it stopped, and
/// checks the size and the hash. A file that doesn't match is deleted.
#[allow(clippy::too_many_arguments)]
fn fetch(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    size: u64,
    sha256: &str,
    model: &str,
    stage: &'static str,
) -> Result<(), String> {
    let mut hasher = Sha256::new();
    let mut have = fs::metadata(path).map_or(0, |m| m.len());
    if have > size {
        let _ = fs::remove_file(path);
        have = 0;
    }
    if have > 0 {
        // The part already on disk goes into the hash first.
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if CANCEL.load(Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    let mut done = have;
    let mut last = Instant::now();
    let tick = |done: u64| {
        report(app, Progress { model: model.into(), stage, done, total: size, message: None });
    };
    tick(done);

    if have < size {
        tauri::async_runtime::block_on(async {
            let mut request = client.get(url);
            if have > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
            }
            let mut response = request.send().await.map_err(network)?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("Сервер ответил {status}"));
            }
            let mut file = if have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
                fs::OpenOptions::new().append(true).open(path).map_err(|e| e.to_string())?
            } else {
                // The server sends the whole file again.
                hasher = Sha256::new();
                done = 0;
                File::create(path).map_err(|e| e.to_string())?
            };
            while let Some(chunk) = response.chunk().await.map_err(network)? {
                if CANCEL.load(Ordering::SeqCst) {
                    return Err("cancelled".into());
                }
                file.write_all(&chunk).map_err(|e| format!("Не записалось на диск: {e}"))?;
                hasher.update(&chunk);
                done += chunk.len() as u64;
                if done > size {
                    break;
                }
                if last.elapsed() >= Duration::from_millis(200) {
                    last = Instant::now();
                    tick(done);
                }
            }
            file.flush().map_err(|e| e.to_string())
        })?;
    }

    report(app, Progress { model: model.into(), stage: "check", done: size, total: size, message: None });
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != size || digest != sha256 {
        let _ = fs::remove_file(path);
        return Err("Файл скачался битым, попробуй ещё раз".into());
    }
    Ok(())
}

fn network(e: reqwest::Error) -> String {
    if e.is_connect() || e.is_timeout() {
        "Связь оборвалась. Нажми ещё раз, докачаю с того же места".into()
    } else {
        format!("Ошибка сети: {e}")
    }
}

/// Unpacks next to the destination, then moves it in, so a half-unpacked build never counts.
fn unpack(archive: &Path, dest: &Path, name: &str) -> Result<(), String> {
    let staging = dest.with_extension("unpacking");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let unpacked = if name.ends_with(".zip") { unzip(file, &staging) } else { untar(file, &staging) };
    unpacked.map_err(|e| format!("Не распаковалось: {e}"))?;
    let server = find_server(&staging).ok_or("В архиве нет llama-server")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&server, fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = server;
    let _ = fs::remove_dir_all(dest);
    fs::rename(&staging, dest).map_err(|e| e.to_string())
}

/// The build carries two dozen tools; the server needs only itself and the libraries.
fn wanted(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    name == SERVER_EXE
        || name == "LICENSE"
        || name.ends_with(".dll")
        || name.ends_with(".dylib")
        || name.contains(".so")
}

#[cfg(windows)]
fn unzip(file: File, dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name refuses paths that climb out of the folder.
        let Some(path) = entry.enclosed_name() else { continue };
        if entry.is_dir() || !wanted(&path) {
            continue;
        }
        let out = dest.join(path);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn unzip(_file: File, _dest: &Path) -> Result<(), String> {
    Err("zip".into())
}

#[cfg(not(windows))]
fn untar(file: File, dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if wanted(&path) {
            // unpack_in refuses paths that climb out of the folder; library symlinks come along.
            entry.unpack_in(dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn untar(_file: File, _dest: &Path) -> Result<(), String> {
    Err("tar.gz".into())
}

pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    let model = model(id).ok_or("Нет такой модели")?;
    if DOWNLOAD.lock().unwrap().as_ref().is_some_and(|p| p.model == id) {
        return Err("Сначала останови загрузку".into());
    }
    let mut slot = SERVER.lock().unwrap();
    while slot.starting {
        slot = READY.wait(slot).unwrap();
    }
    if slot.server.as_ref().is_some_and(|s| s.model == id) {
        stop(&mut slot.server);
    }
    drop(slot);
    let path = model_path(app, model)?;
    for file in [path.clone(), path.with_extension("gguf.part")] {
        match fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

// ---------- the server ----------

struct Server {
    child: Child,
    port: u16,
    model: String,
    used: Instant,
    /// Holds the process id, so the next run can stop a server this one left behind.
    pid_file: PathBuf,
}

/// The running server, and whether one is being started right now. The lock is
/// held only to look and to swap: loading a model takes up to two minutes, and
/// quitting, the idle timer and deleting a model must not wait for that.
struct Slot {
    server: Option<Server>,
    starting: bool,
}

static SERVER: Mutex<Slot> = Mutex::new(Slot { server: None, starting: false });
/// Rung when a start is over, whichever way it went.
static READY: Condvar = Condvar::new();
/// The app is quitting: a start in progress gives up and stops its process.
static QUITTING: AtomicBool = AtomicBool::new(false);
const IDLE: Duration = Duration::from_secs(10 * 60);

fn stop(server: &mut Option<Server>) {
    if let Some(mut s) = server.take() {
        let _ = s.child.kill();
        let _ = s.child.wait();
        let _ = fs::remove_file(&s.pid_file);
    }
}

/// Stops the server; called when the app quits. A start in progress sees
/// `QUITTING` within a quarter of a second and stops its own process.
pub fn shutdown() {
    QUITTING.store(true, Ordering::SeqCst);
    let mut slot = SERVER.lock().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while slot.starting {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        slot = READY.wait_timeout(slot, left).unwrap().0;
    }
    stop(&mut slot.server);
}

/// Marks the server as just used, so the idle timer starts over.
pub fn touch() {
    if let Some(server) = SERVER.lock().unwrap().server.as_mut() {
        server.used = Instant::now();
    }
}

/// The address of a running server with the model, starting it if needed.
pub fn ensure(app: &AppHandle, id: &str) -> Result<String, String> {
    let model = model(id).ok_or("Выбери модель в настройках")?;
    let mut slot = SERVER.lock().unwrap();
    // Another request is starting it: wait instead of starting a second one.
    while slot.starting {
        slot = READY.wait(slot).unwrap();
    }
    if let Some(s) = slot.server.as_mut() {
        if s.model == model.id && matches!(s.child.try_wait(), Ok(None)) {
            s.used = Instant::now();
            return Ok(address(s.port));
        }
    }
    stop(&mut slot.server);
    if QUITTING.load(Ordering::SeqCst) {
        return Err(QUIT.into());
    }
    slot.starting = true;
    drop(slot);

    let started = launch(app, model);

    let mut slot = SERVER.lock().unwrap();
    slot.starting = false;
    READY.notify_all();
    let mut s = started?;
    if QUITTING.load(Ordering::SeqCst) {
        // `shutdown` has been and gone; nobody else would stop this one.
        stop(&mut Some(s));
        return Err(QUIT.into());
    }
    s.used = Instant::now();
    let port = s.port;
    slot.server = Some(s);
    drop(slot);
    watch_idle();
    Ok(address(port))
}

const QUIT: &str = "Airtype закрывается";
const DIED: &str = "Модель не запустилась. Что пошло не так, написано в server.log в папке с моделями";

/// Starts the server for the model: on the GPU first, and on the processor when
/// the GPU build can't load it, after which the rest of the run skips the GPU.
fn launch(app: &AppHandle, model: &'static Model) -> Result<Server, String> {
    if !downloaded(app, model) {
        return Err("Сначала скачай модель в настройках, раздел «Модель»".into());
    }
    let root = root(app)?;
    reap_orphan(&root);
    let exe = runtime_dir(app)
        .ok()
        .and_then(|dir| find_server(&dir))
        .ok_or("Сначала скачай модель в настройках")?;
    let model_file = model_path(app, model)?;
    // One log per start: the GPU attempt's reasons stay above the processor's.
    let log = root.join("server.log");
    let mut fresh_log = true;
    let mut gpu = GPU.load(Ordering::SeqCst);
    // A port taken in the moment between finding it free and the server binding it.
    let mut port_retries = 2;
    loop {
        let result = start(&exe, &model_file, &log, fresh_log, gpu);
        fresh_log = false;
        match result {
            Ok(mut s) => {
                s.model = model.id.into();
                return Ok(s);
            }
            Err(Failed::Port) if port_retries > 0 => port_retries -= 1,
            // A driver that can't load the model, or stalls on it: the processor can.
            Err(Failed::Exited | Failed::Slow) if gpu => {
                GPU.store(false, Ordering::SeqCst);
                gpu = false;
            }
            Err(Failed::Port | Failed::Exited) => return Err(DIED.into()),
            Err(Failed::Slow) => return Err("Модель слишком долго загружается".into()),
            Err(Failed::Other(message)) => return Err(message),
        }
    }
}

/// Whether to offload the model to the GPU. Only builds that have one to
/// offload to; cleared after the GPU build failed to load the model.
static GPU: AtomicBool = AtomicBool::new(cfg!(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(any(windows, target_os = "linux"), target_arch = "x86_64"),
)));

enum Failed {
    /// The server couldn't bind its port.
    Port,
    /// The server quit on its own: it couldn't load the model.
    Exited,
    /// Still not loaded after two minutes.
    Slow,
    /// Anything else, with what to tell the user.
    Other(String),
}

/// Starts the server and waits until the model is loaded.
fn start(exe: &Path, model_file: &Path, log: &Path, fresh_log: bool, gpu: bool) -> Result<Server, Failed> {
    let port = free_port().ok_or_else(|| Failed::Other("Не нашёл свободный порт для модели".into()))?;
    // What the server says goes to a log next to the models, for when it won't start.
    let mut options = fs::OpenOptions::new();
    options.create(true);
    if fresh_log {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    let log_file = options.open(log).ok();
    let log_from = log_file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
    let mut command = Command::new(exe);
    command
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .arg("--model")
        .arg(model_file)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        // One request at a time, with room for a long text and its answer.
        .args(["--ctx-size", &CONTEXT.to_string(), "--parallel", "1", "--jinja", "--no-webui"])
        // As many layers as fit in video memory (all of them for these
        // models on most cards), or none.
        .args(["--n-gpu-layers", if gpu { "auto" } else { "0" }])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log_file.map_or_else(Stdio::null, Stdio::from));
    let child = platform::spawn(command).map_err(|e| Failed::Other(format!("Модель не запустилась: {e}")))?;
    platform::adopt(&child);
    // Written at once: a crash while the model loads must not leave it unknown.
    let pid_file = log.with_file_name("server.pid");
    let _ = fs::write(&pid_file, child.id().to_string());
    let mut s = Server { child, port, model: String::new(), used: Instant::now(), pid_file };
    fn kill(s: &mut Server, failed: Failed) -> Failed {
        let _ = s.child.kill();
        let _ = s.child.wait();
        let _ = fs::remove_file(&s.pid_file);
        failed
    }

    // A server that can't bind its port quits at once; give it the moment, so
    // another program on that port isn't taken for it.
    thread::sleep(Duration::from_millis(200));
    // Loading the weights takes a few seconds, longer from a slow disk.
    let start = Instant::now();
    loop {
        if QUITTING.load(Ordering::SeqCst) {
            return Err(kill(&mut s, Failed::Other(QUIT.into())));
        }
        if !matches!(s.child.try_wait(), Ok(None)) {
            let _ = fs::remove_file(&s.pid_file);
            return Err(exit_reason(log, log_from));
        }
        if healthy(port) && matches!(s.child.try_wait(), Ok(None)) {
            return Ok(s);
        }
        if start.elapsed() > Duration::from_secs(120) {
            return Err(kill(&mut s, Failed::Slow));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Tokens the server holds for the text and the answer together.
const CONTEXT: u32 = 16384;

/// Why the server quit, from what it wrote to the log after `from`.
fn exit_reason(log: &Path, from: u64) -> Failed {
    let Ok(mut file) = File::open(log) else { return Failed::Exited };
    let mut tail = Vec::new();
    if file.seek(SeekFrom::Start(from)).is_err() || file.read_to_end(&mut tail).is_err() {
        return Failed::Exited;
    }
    // llama.cpp: "couldn't bind HTTP server socket, hostname: 127.0.0.1, port: ..."
    if String::from_utf8_lossy(&tail).contains("couldn't bind") {
        Failed::Port
    } else {
        Failed::Exited
    }
}

/// A server that an earlier run left behind when it crashed or was force-quit.
/// Windows ends it with the app through a job; Linux by the parent-death signal.
fn reap_orphan(root: &Path) {
    static DONE: Once = Once::new();
    DONE.call_once(|| {
        let file = root.join("server.pid");
        let Some(pid) = fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
            return;
        };
        let _ = fs::remove_file(&file);
        platform::kill_stale(pid);
    });
}

fn address(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn free_port() -> Option<u16> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?.local_addr().ok().map(|a| a.port())
}

/// `/health` answers 200 once the model is loaded and 503 while it loads.
fn healthy(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n").is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    let n = stream.read(&mut head).unwrap_or(0);
    head[..n].starts_with(b"HTTP/1.1 200") || head[..n].starts_with(b"HTTP/1.0 200")
}

/// Stops the server after ten idle minutes, giving its memory back.
fn watch_idle() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        thread::spawn(|| loop {
            thread::sleep(Duration::from_secs(30));
            let mut slot = SERVER.lock().unwrap();
            if slot.server.as_ref().is_some_and(|s| s.used.elapsed() > IDLE) {
                stop(&mut slot.server);
            }
        });
    });
}

#[cfg(windows)]
mod platform {
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    use std::process::{Child, Command};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn spawn(mut command: Command) -> std::io::Result<Child> {
        command.creation_flags(CREATE_NO_WINDOW).spawn()
    }

    /// A job that ends the server when Airtype ends, even if it crashes.
    fn job() -> Option<HANDLE> {
        static JOB: OnceLock<usize> = OnceLock::new();
        let job = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            job as usize
        });
        (job != 0).then_some(job as HANDLE)
    }

    pub fn adopt(child: &Child) {
        if let Some(job) = job() {
            unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };
        }
    }

    /// The job already ended it along with the app that started it.
    pub fn kill_stale(_pid: u32) {}
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    use std::sync::{mpsc, OnceLock};

    type Request = (Command, mpsc::Sender<io::Result<Child>>);

    /// The kernel stops the server when its parent ends, even if it crashes. The
    /// "parent" there is the thread that started it, not the process, so every
    /// server is started from one thread that lives as long as the app.
    pub fn spawn(mut command: Command) -> io::Result<Child> {
        static LAUNCHER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
        let parent = std::process::id();
        unsafe {
            command.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                // The app went away before the signal was armed.
                if libc::getppid() as u32 != parent {
                    libc::_exit(0);
                }
                Ok(())
            });
        }
        let launcher = LAUNCHER.get_or_init(|| {
            let (sender, requests) = mpsc::channel::<Request>();
            let _ = std::thread::Builder::new().name("airtype-launcher".into()).spawn(move || {
                for (mut command, reply) in requests {
                    let _ = reply.send(command.spawn());
                }
            });
            sender
        });
        let (reply, child) = mpsc::channel();
        let gone = || io::Error::other("не запустился поток для модели");
        launcher.send((command, reply)).map_err(|_| gone())?;
        child.recv().map_err(|_| gone())?
    }

    pub fn adopt(_child: &Child) {}

    pub fn kill_stale(pid: u32) {
        super::kill_if_server(pid);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use std::process::{Child, Command};

    pub fn spawn(mut command: Command) -> std::io::Result<Child> {
        command.spawn()
    }

    pub fn adopt(_child: &Child) {}

    /// Nothing ends the server with a crashed app here, so the next run does.
    pub fn kill_stale(pid: u32) {
        super::kill_if_server(pid);
    }
}

/// Stops the process if it still is a llama-server: the id may belong to
/// another program by now.
#[cfg(unix)]
fn kill_if_server(pid: u32) {
    let Ok(out) = Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output() else { return };
    if String::from_utf8_lossy(&out.stdout).trim().ends_with(SERVER_EXE) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_are_known() {
        assert!(model("qwen3.5-2b").is_some());
        assert!(model("nope").is_none());
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64);
            assert!(m.url.ends_with(m.file));
        }
        assert_eq!(crate::config::LocalConfig::default().model, MODELS[0].id);
    }

    #[test]
    fn old_runtimes_go_but_not_the_rest() {
        let root = std::env::temp_dir().join(format!("airtype-test-runtimes-{}", std::process::id()));
        let keep = root.join("llama-b11669-bin-win-vulkan-x64");
        for dir in [&keep, &root.join("llama-b11669"), &root.join("models")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(root.join("llama-b11669-bin-win-vulkan-x64.zip.part"), b"").unwrap();
        remove_old_runtimes(&root, &keep);
        assert!(keep.is_dir());
        assert!(root.join("models").is_dir());
        assert!(root.join("llama-b11669-bin-win-vulkan-x64.zip.part").is_file());
        assert!(!root.join("llama-b11669").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_server_in_nested_folders() {
        let dir = std::env::temp_dir().join(format!("airtype-test-local-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("build").join("bin")).unwrap();
        fs::write(dir.join("build").join("bin").join(SERVER_EXE), b"").unwrap();
        assert_eq!(find_server(&dir), Some(dir.join("build").join("bin").join(SERVER_EXE)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unpacks_only_the_server() {
        assert!(wanted(Path::new("llama-b11669/llama-server")) || cfg!(windows));
        assert!(wanted(Path::new("llama-b11669/libllama.so.0.7.0")));
        assert!(wanted(Path::new("llama-b11669/libggml-metal.0.dylib")));
        assert!(wanted(Path::new("ggml-cpu-zen4.dll")));
        assert!(!wanted(Path::new("llama-b11669/llama-bench")));
        assert!(!wanted(Path::new("llama-b11669/")));
    }

    #[test]
    fn health_needs_a_server() {
        let port = free_port().unwrap();
        assert!(!healthy(port));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf);
            stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}").unwrap();
        });
        assert!(healthy(port));
        server.join().unwrap();
    }
}
