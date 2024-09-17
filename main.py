import argparse
import glob
import os
import sys

import requests
from rich.console import Console
from rich.panel import Panel
from rich.progress import (BarColumn, DownloadColumn, Progress, TextColumn,
                           TimeRemainingColumn, TransferSpeedColumn)

github_repo = "dest4590/CollapseLoader"

console = Console(highlight=False)

parser = argparse.ArgumentParser(description="CollapseLoader")
parser.add_argument("--prelease", action="store_true", help="Download pre-release")
args, unknown = parser.parse_known_args()

class Updater:
    """Class to update the program"""

    def __init__(self, github_repo: str):
        console.print(Panel(f"[bold blue]Updater for {github_repo}[/]", expand=False))
        
        self.github_repo = github_repo
        self.latest_release = self.get_latest_release()

    def get_latest_release(self) -> bool:
        """Get the latest release from the GitHub API"""
        if args.prelease:
            url = f"https://api.github.com/repos/{github_repo}/releases"
        else:
            url = f"https://api.github.com/repos/{github_repo}/releases/latest"

        try:
            response = requests.get(url)
            response.raise_for_status()

            if response.status_code == 429:
                console.print(
                    "[bold red]Error:[/] Rate limit exceeded! Please wait a while before trying again."
                )
                return False

            elif response.status_code != 200:
                console.print("[bold red]Error:[/] Failed to fetch latest release!")
                return False

        except requests.exceptions.RequestException as e:
            console.print("[bold red]Error:[/] Failed to fetch latest release!")
            print(e)
            return False

        if args.prelease:
            console.print("[blue]Fetching latest pre-release...[/]")
            for release in response.json():
                if release['prerelease']:
                    return release
            console.print("[bold red]Error:[/] No pre-release found!")
            return False
        else:
            return response.json()

    def download_latest_release(self):
        """Download the latest release from the GitHub API"""
        release = self.latest_release
        if not release:
            return

        asset = release["assets"][0]
        if args.prelease:
            console.print(f"[blue]Downloading latest pre-release:[/]: {asset['name']}")
        else:
            console.print(f"[blue]Downloading latest release:[/] {asset['name']}")

        with Progress(
            TextColumn("[bold blue]{task.description}", justify="right"),
            BarColumn(),
            "[progress.percentage]{task.percentage:>3.1f}%",
            "•",
            DownloadColumn(),
            "•",
            TransferSpeedColumn(),
            "•",
            TimeRemainingColumn(),
        ) as progress:
            task = progress.add_task(
                "[green]Downloading", filename=asset["name"], total=asset["size"]
            )

            download_url = asset["browser_download_url"]
            with requests.get(download_url, stream=True) as response:
                response.raise_for_status()
                with open(asset["name"], "wb") as file:
                    for chunk in response.iter_content(chunk_size=8192):
                        file.write(chunk)
                        progress.update(task, advance=len(chunk))

        console.print(f"[green]Downloaded successfully:[/] {asset['name']}")

    def delete_old_releases(self, exclude_file=None):
        """Delete old releases of CollapseLoader, optionally excluding a file"""
        old_releases = glob.glob("CollapseLoader*")
        for old_release in old_releases:
            if old_release != exclude_file and os.path.isfile(old_release):
                os.remove(old_release)
                console.print(f"[yellow]Deleted old release:[/] {old_release}")

    def main(self):
        """Main function to update the program"""
        if self.latest_release:
            latest_release_filename = self.latest_release['assets'][0]['name']

            self.delete_old_releases(exclude_file=latest_release_filename)

            if not os.path.exists(latest_release_filename):
                self.download_latest_release()
            else:
                console.print("[yellow]Latest version already downloaded.[/]")

            console.print("[blue]Starting CollapseLoader...[/]")
            command = [latest_release_filename] + sys.argv[1:]
            os.system(" ".join(command))

# Prevent running the updater when pyinstaller is building the .exe file
if '_child.py' not in sys.argv[0]:
    Updater(github_repo).main()
