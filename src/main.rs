use console::style;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use std::{
    cmp::min,
    env,
    fs::{self, File},
    io::{self, Write},
    path::Path,
    process::{exit, Command},
};

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    browser_download_url: String,
    size: u64,
}

async fn get_download_url(client: &Client) -> Result<(String, u64), Box<dyn std::error::Error>> {
    let response = client
        .get("https://api.github.com/repos/dest4590/CollapseLoader/releases/latest")
        .send()
        .await?;

    if !response.status().is_success() {
        let error_message = format!(
            "API request failed with status code: {}. Response body: {}",
            response.status(),
            response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to get response body".to_string())
        );
        return Err(error_message.into());
    }

    let release: Release = response.json().await?;

    match release.assets.first() {
        Some(asset) => Ok((asset.browser_download_url.clone(), asset.size)),
        None => Err("No assets found in the release".into()),
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

fn start_loader(file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", style("Starting CollapseLoader...").green());

    let mut command = Command::new(file_path);

    command.stdin(std::process::Stdio::inherit());
    command.stdout(std::process::Stdio::inherit());
    command.stderr(std::process::Stdio::inherit());

    let output = command.output()?;

    if !output.status.success() {
        exit(0);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().user_agent("CollapseUpdater").build()?;
    let (download_url, total_size) = get_download_url(&client).await?;
    let filename = download_url[download_url.rfind('/').unwrap_or(0) + 1..].to_string();

    let panel_width = 40;
    let welcome_text = format!(
        "╭{}╮\n│{:^width$}│\n╰{}╯",
        "─".repeat(panel_width - 2),
        style("Updater for CollapseLoader").bold().blue(),
        "─".repeat(panel_width - 2),
        width = panel_width - 2
    );
    println!("{}", welcome_text);

    if let Err(err) = delete_old(&filename) {
        eprintln!("{} {}", style("Error deleting files:").red(), err);
    }

    if is_file_already_downloaded(&filename, total_size) {
        start_loader(&filename)?;
        return Ok(());
    }

    println!(
        "{} {}",
        style("Downloading latest release:").blue(),
        filename
    );

    let res = client.get(&download_url).send().await?;

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-")
    );
    let mut downloaded: u64 = 0;
    let mut file = File::create(&filename)?;

    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = &item?;
        file.write_all(&chunk)?;
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
    start_loader(&filename)?;
    Ok(())
}
