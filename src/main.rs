#[cfg(windows)]
use std::os::windows::process::CommandExt;

use std::{
    cmp::min,
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use console::{style, Emoji};
use directories::ProjectDirs;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;

const ORG_NAME: &str = "dest4590";
const APP_NAME: &str = "CollapseLoader";
const REPO_OWNER: &str = "dest4590";
const REPO_NAME: &str = "CollapseLoader";

static LOOKING_GLASS: Emoji<'_, '_> = Emoji("🔍", "•");
static TRUCK: Emoji<'_, '_> = Emoji("🚚", "•");
static DISK: Emoji<'_, '_> = Emoji("💾", "•");
static ROCKET: Emoji<'_, '_> = Emoji("🚀", "•");
static SPARKLE: Emoji<'_, '_> = Emoji("✨", "•");
static ERROR_ICON: Emoji<'_, '_> = Emoji("❌", "x");

struct Messages {
    header_title: &'static str,

    init_path: &'static str,

    checking_updates: &'static str,
    retrying: &'static str,
    found_version: &'static str,

    cleaning_up: &'static str,
    deleted_old: &'static str,

    download_start: &'static str,
    already_updated: &'static str,
    update_success: &'static str,
    file_locked_error: &'static str,

    launching: &'static str,
    good_game: &'static str,
    closing_in: &'static str,

    press_enter: &'static str,
    download_complete: &'static str,
}

const EN_MESSAGES: Messages = Messages {
    header_title: "Updater for",
    init_path: "Installation path:",
    checking_updates: "Checking for updates...",
    retrying: "Connection failed, retrying in 2s...",
    found_version: "Found remote version:",
    cleaning_up: "Cleaning up old versions...",
    deleted_old: "Removed old file:",
    download_start: "Downloading new version...",
    already_updated: "Latest version is already installed.",
    update_success: "Update installed successfully!",
    file_locked_error:
        "Could not replace file. Is the loader/game still running? Close it and try again.",
    launching: "Launching application...",
    good_game: "All done. Have a good game!",
    closing_in: "Closing in 3 seconds...",
    press_enter: "Press Enter to exit...",
    download_complete: "Download complete",
};

const RU_MESSAGES: Messages = Messages {
    header_title: "Обновление для",
    init_path: "Путь установки:",
    checking_updates: "Проверка обновлений...",
    retrying: "Сбой сети, повторная попытка через 2с...",
    found_version: "Найдена версия:",
    cleaning_up: "Очистка старых версий...",
    deleted_old: "Удален старый файл:",
    download_start: "Загрузка новой версии...",
    already_updated: "Последняя версия уже установлена.",
    update_success: "Обновление успешно установлено!",
    file_locked_error:
        "Не удалось заменить файл. Лоадер или игра все еще запущены? Закройте их и повторите.",
    launching: "Запуск приложения...",
    good_game: "Готово. Приятной игры!",
    closing_in: "Закрытие через 3 секунды...",
    press_enter: "Нажмите Enter для выхода...",
    download_complete: "Загрузка завершена",
};

fn get_messages() -> &'static Messages {
    let locale = sys_locale::get_locale()
        .unwrap_or_else(|| "en-US".to_string())
        .to_lowercase();
    if locale.starts_with("ru") || locale.starts_with("uk") || locale.starts_with("be") {
        &RU_MESSAGES
    } else {
        &EN_MESSAGES
    }
}

#[derive(Deserialize, Debug)]
struct Release {
    assets: Vec<Asset>,
    prerelease: bool,
    tag_name: String,
}

#[derive(Deserialize, Debug)]
struct Asset {
    browser_download_url: String,
    size: u64,
    name: String,
}

fn get_install_directory(msgs: &Messages) -> Result<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("com", ORG_NAME, APP_NAME) {
        let data_dir = proj_dirs.data_dir();

        println!("   {} {}", style("└").dim(), msgs.init_path);
        println!("     {}", style(data_dir.display()).dim().italic());

        if !data_dir.exists() {
            fs::create_dir_all(data_dir)
                .with_context(|| format!("Failed to create directory: {:?}", data_dir))?;
        }
        Ok(data_dir.to_path_buf())
    } else {
        bail!("Could not determine home directory.");
    }
}

async fn get_release_info(
    client: &Client,
    pre_release: bool,
    msgs: &Messages,
) -> Result<(String, u64, String, String)> {
    let base_url = format!(
        "https://api.github.com/repos/{}/{}/releases",
        REPO_OWNER, REPO_NAME
    );
    let url = if pre_release {
        base_url
    } else {
        format!("{}/latest", base_url)
    };

    println!("{} {}", LOOKING_GLASS, style(msgs.checking_updates).bold());

    let mut attempt = 0;
    let max_attempts = 3;
    let response = loop {
        match client.get(&url).send().await {
            Ok(resp) => break resp,
            Err(e) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(e.into());
                }
                println!(
                    "   {} {} ({}/{})",
                    style("└").red(),
                    msgs.retrying,
                    attempt,
                    max_attempts
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    };

    if !response.status().is_success() {
        bail!(
            "GitHub API failed: {} - {}",
            response.status(),
            response.text().await?
        );
    }

    let (target_asset, tag_name) = if pre_release {
        let releases: Vec<Release> = response.json().await?;
        let rel = releases
            .into_iter()
            .find(|r| r.prerelease)
            .context("No pre-release found")?;
        (
            rel.assets.into_iter().next().context("No assets found")?,
            rel.tag_name,
        )
    } else {
        let rel: Release = response.json().await?;
        (
            rel.assets.into_iter().next().context("No assets found")?,
            rel.tag_name,
        )
    };

    println!(
        "   {} {} {}",
        style("└").green(),
        msgs.found_version,
        style(&tag_name).cyan()
    );

    Ok((
        target_asset.browser_download_url,
        target_asset.size,
        target_asset.name,
        tag_name,
    ))
}

fn cleanup_old_versions(install_dir: &Path, current_filename: &str, msgs: &Messages) -> Result<()> {
    println!("{} {}", TRUCK, style(msgs.cleaning_up).bold());

    let entries = fs::read_dir(install_dir)?
        .filter_map(|res| res.ok())
        .filter(|e| e.path().is_file());

    let mut found_garbage = false;

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.ends_with(".tmp") {
            let _ = fs::remove_file(&path);
            continue;
        }

        if name.starts_with("CollapseLoader") && name.ends_with(".exe") && name != current_filename
        {
            if let Ok(_) = fs::remove_file(&path) {
                println!("   {} {} {}", style("└").red(), msgs.deleted_old, name);
                found_garbage = true;
            }
        }
    }

    if !found_garbage {
        println!("   {} {}", style("└").dim(), style("Clean.").dim());
    }
    Ok(())
}

async fn download_file(
    client: &Client,
    url: &str,
    total_size: u64,
    target_path: &Path,
    msgs: &Messages,
) -> Result<()> {
    let tmp_path = target_path.with_extension("tmp");

    println!("{} {}", DISK, style(msgs.download_start).bold());

    let res = client.get(url).send().await?;
    let mut stream = res.bytes_stream();

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?
            .progress_chars("#>-"),
    );

    let mut file = File::create(&tmp_path).context("Failed to create temporary file")?;
    let mut downloaded: u64 = 0;

    while let Some(item) = stream.next().await {
        let chunk = item.context("Network error during download")?;
        file.write_all(&chunk).context("Disk error while writing")?;

        downloaded = min(downloaded + (chunk.len() as u64), total_size);
        pb.set_position(downloaded);
    }

    pb.finish_with_message(msgs.download_complete);

    if target_path.exists() {
        if let Err(e) = fs::remove_file(target_path) {
            if e.kind() == io::ErrorKind::PermissionDenied {
                bail!("{}", msgs.file_locked_error);
            }
            return Err(e).context("Failed to delete old file");
        }
    }

    fs::rename(&tmp_path, target_path).context("Failed to rename file")?;

    Ok(())
}

fn start_loader(executable_path: &Path, msgs: &Messages) -> Result<()> {
    println!("{} {}", ROCKET, style(msgs.launching).bold());

    if !executable_path.exists() {
        bail!("Executable not found at: {:?}", executable_path);
    }

    let mut command = Command::new(executable_path);

    for arg in env::args().skip(1) {
        command.arg(arg);
    }

    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        const DETACHED_PROCESS: u32 = 0x00000008;
        command.creation_flags(DETACHED_PROCESS);
    }

    command.spawn().context("Failed to launch loader")?;

    Ok(())
}

fn is_up_to_date(file_path: &Path, expected_size: u64) -> bool {
    if file_path.exists() {
        if let Ok(metadata) = fs::metadata(file_path) {
            return metadata.len() == expected_size;
        }
    }
    false
}

#[tokio::main]
async fn main() {
    let msgs = get_messages();

    let panel_width = 52;
    println!("\n╭{}╮", "─".repeat(panel_width));
    println!(
        "│{:^width$}│",
        style(format!(
            "{} {} v{}",
            msgs.header_title,
            APP_NAME,
            env!("CARGO_PKG_VERSION")
        ))
        .bold()
        .blue(),
        width = panel_width
    );
    println!("╰{}╯\n", "─".repeat(panel_width));

    if let Err(e) = run(msgs).await {
        println!("");
        eprintln!("{} {}", ERROR_ICON, style("CRITICAL ERROR").red().bold());
        eprintln!("   {} {}", style("└").red(), style(e.to_string()).white());

        for cause in e.chain().skip(1) {
            eprintln!("     {} Caused by: {}", style("└").red(), cause);
        }

        println!("\n{}", msgs.press_enter);
        let _ = std::io::stdin().read_line(&mut String::new());
        std::process::exit(1);
    }
}

async fn run(msgs: &Messages) -> Result<()> {
    let pre_release = env::args().any(|arg| arg == "--prerelease");

    let install_dir = get_install_directory(msgs)?;
    let client = Client::builder()
        .user_agent(format!("{}Updater", APP_NAME))
        .build()?;

    let (download_url, size, filename, _) = get_release_info(&client, pre_release, msgs).await?;
    let target_path = install_dir.join(&filename);

    cleanup_old_versions(&install_dir, &filename, msgs)?;

    if is_up_to_date(&target_path, size) {
        println!("{} {}", SPARKLE, style(msgs.already_updated).green());
    } else {
        download_file(&client, &download_url, size, &target_path, msgs).await?;
        println!("{} {}", SPARKLE, style(msgs.update_success).green());
    }

    println!("");

    start_loader(&target_path, msgs)?;

    println!("\n{}", style(msgs.good_game).cyan().bold());
    println!("{}", style(msgs.closing_in).dim());

    tokio::time::sleep(Duration::from_secs(3)).await;

    Ok(())
}
