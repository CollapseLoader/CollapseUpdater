use console::style;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use std::{
    cmp::min,
    env,
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, Write},
    path::Path,
    process::Command,
};

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
    prerelease: bool,
}

#[derive(Deserialize)]
struct Asset {
    browser_download_url: String,
    size: u64,
}

#[derive(Debug)]
enum UpdaterError {
    ApiRequestError(String),
    FileOperationError(String),
    CommandExecutionError(String),
    NoPreReleaseFound,
}

impl fmt::Display for UpdaterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UpdaterError::ApiRequestError(msg) => write!(f, "API request error: {}", msg),
            UpdaterError::FileOperationError(msg) => write!(f, "File operation error: {}", msg),
            UpdaterError::CommandExecutionError(msg) => {
                write!(f, "Command execution error: {}", msg)
            }
            UpdaterError::NoPreReleaseFound => write!(f, "No pre-release found!"),
        }
    }
}

impl Error for UpdaterError {}

const GITHUB_REPO: &str = "dest4590/CollapseLoader";

async fn get_download_url(
    client: &Client,
    pre_release: bool,
) -> Result<(String, u64), UpdaterError> {
    let url = if pre_release {
        format!("https://api.github.com/repos/{}/releases", GITHUB_REPO)
    } else {
        format!(
            "https://api.github.com/repos/{}/releases/latest",
            GITHUB_REPO
        )
    };

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|err| UpdaterError::ApiRequestError(err.to_string()))?;

    if !response.status().is_success() {
        let error_message = format!(
            "API request failed with status code: {}. Response body: {}",
            response.status(),
            response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to get response body".to_string()),
        );
        return Err(UpdaterError::ApiRequestError(error_message));
    }

    if pre_release {
        let releases: Vec<Release> = response
            .json()
            .await
            .map_err(|err| UpdaterError::ApiRequestError(err.to_string()))?;

        for release in releases {
            if release.prerelease {
                if let Some(asset) = release.assets.first() {
                    return Ok((asset.browser_download_url.clone(), asset.size));
                }
            }
        }

        return Err(UpdaterError::NoPreReleaseFound);
    } else {
        let release: Release = response
            .json()
            .await
            .map_err(|err| UpdaterError::ApiRequestError(err.to_string()))?;

        match release.assets.first() {
            Some(asset) => Ok((asset.browser_download_url.clone(), asset.size)),
            None => Err(UpdaterError::ApiRequestError(
                "No assets found in the release".to_string(),
            )),
        }
    }
}

fn is_file_already_downloaded(file_path: &str, expected_size: u64) -> bool {
    if Path::new(file_path).exists() {
        if let Ok(metadata) = std::fs::metadata(file_path) {
            if metadata.len() == expected_size {
                println!(
                    "{} {}",
                    style("Latest version already downloaded:").yellow(),
                    file_path
                );
                return true;
            }
        }
    }
    false
}

fn delete_old(exclude: &str) -> Result<(), io::Error> {
    let folder = env::current_dir()?;
    let entries = fs::read_dir(&folder)?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|filename| {
            filename != exclude
                && filename.starts_with("CollapseLoader")
                && filename.ends_with(".exe")
        })
        .collect::<Vec<String>>();

    for filename in entries {
        let file_path = folder.join(&filename);
        match fs::remove_file(&file_path) {
            Ok(_) => println!("{} {}", style("Deleted:").red(), filename),
            Err(e) => eprintln!("{} {}: {}", style("Failed to delete").red(), filename, e),
        }
    }

    Ok(())
}

fn start_loader(file_path: &str) -> Result<(), UpdaterError> {
    println!("{}", style("Starting CollapseLoader...\n").green());

    let full_path = std::env::current_dir().unwrap().join(file_path);

    let mut command = Command::new(full_path);

    for arg in std::env::args().skip(1) {
        command.arg(arg);
    }

    command.stdin(std::process::Stdio::inherit());
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());

    let output = command
        .output()
        .map_err(|err| UpdaterError::CommandExecutionError(err.to_string()))?;

    if !output.status.success() {
        return Err(UpdaterError::CommandExecutionError(format!(
            "Process exited with code: {}",
            output.status
        )));
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pre_release = std::env::args().any(|arg| arg == "--prerelease");

    let client = Client::builder().user_agent("CollapseUpdater").build()?;
    let (download_url, total_size) = get_download_url(&client, pre_release).await?;
    let filename = download_url[download_url.rfind('/').unwrap_or(0) + 1..].to_string();

    let panel_width = 40;
    let welcome_text = format!(
        "╭{}╮\n│{:^width$}│\n╰{}╯\n",
        "─".repeat(panel_width - 2),
        style(format!(
            "Updater for CollapseLoader ({})",
            env!("CARGO_PKG_VERSION")
        ))
        .bold()
        .blue(),
        "─".repeat(panel_width - 2),
        width = panel_width - 2
    );
    print!("{}", welcome_text);

    if let Err(err) = delete_old(&filename) {
        eprintln!("{} {}", style("Error deleting files:").red(), err);
    }

    if is_file_already_downloaded(&filename, total_size) {
        start_loader(&filename)?;
        return Ok(());
    }

    println!(
        "{} {}",
        style("\nDownloading latest release:").blue(),
        filename
    );

    let res = client
        .get(&download_url)
        .send()
        .await
        .map_err(|err| UpdaterError::ApiRequestError(err.to_string()))?;

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    pb.set_message("Downloading...");

    let mut downloaded: u64 = 0;
    let mut file = File::create(&filename).map_err(|err| {
        UpdaterError::FileOperationError(format!("Failed to create file: {}", err))
    })?;

    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|err| {
            UpdaterError::ApiRequestError(format!("Error downloading file: {}", err))
        })?;
        file.write_all(&chunk).map_err(|err| {
            UpdaterError::FileOperationError(format!("Error writing to file: {}", err))
        })?;

        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(downloaded);
    }

    pb.finish_with_message(format!(
        "{} {}",
        style("Downloaded successfully:").green().bold(),
        filename
    ));
    
    drop(file);
    drop(stream);

    if let Err(err) = start_loader(&filename) {
        eprintln!("Error: {}", err);
    }

    Ok(())
}
