use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
static LOG_MUTEX: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
pub struct DiagnosticLog {
    pub path: String,
    pub contents: String,
}

fn log_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not locate the local application data directory".to_string())?;
    Ok(data_dir.join("InterviewBuddy").join("logs").join("app.log"))
}

fn rotate_if_needed(path: &PathBuf) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() >= MAX_LOG_BYTES => {
            let previous_path = path.with_file_name("app.previous.log");
            if previous_path.exists() {
                fs::remove_file(&previous_path)?;
            }
            fs::rename(path, previous_path)?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    Ok(())
}

pub fn initialize() -> Result<(), String> {
    let path = log_path()?;
    let _guard = LOG_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let parent = path
        .parent()
        .ok_or_else(|| "Could not determine the diagnostics directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create diagnostics directory: {error}"))?;
    rotate_if_needed(&path)
        .map_err(|error| format!("Could not rotate diagnostics log: {error}"))?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Could not create diagnostics log: {error}"))?;
    Ok(())
}

pub fn record(level: &str, message: impl AsRef<str>) {
    let _guard = match LOG_MUTEX.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let result = (|| -> io::Result<()> {
        let path = log_path().map_err(io::Error::other)?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid diagnostics log path")
        })?;
        fs::create_dir_all(parent)?;
        rotate_if_needed(&path)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "[{timestamp}] [{level}] {}", message.as_ref())
    })();

    if let Err(error) = result {
        eprintln!("Could not write diagnostics log: {error}");
    }
}

pub fn read() -> Result<DiagnosticLog, String> {
    let _guard = LOG_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = log_path()?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read diagnostics log: {error}"))?;
    Ok(DiagnosticLog {
        path: path.display().to_string(),
        contents,
    })
}
