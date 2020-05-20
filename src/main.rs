use discord_finder::*;
use clap::clap_app;
use pkg_version::*;
use serde::{Serialize, Deserialize};
use std::{thread::sleep, time::{Duration, SystemTime, UNIX_EPOCH}, fs::File};
use meilisearch_sdk::{client::Client, document::Document};
use progress_bar::{progress_bar::ProgressBar, color::*};

const MAJOR: u32 = pkg_version_major!();
const MINOR: u32 = pkg_version_minor!();
const PATCH: u32 = pkg_version_patch!();

fn ask(question: &str) -> bool {
    println!("{} (Yes / No)", question);
    loop { 
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).unwrap();
        match answer.trim() {
            "y" | "Y" | "Yes" | "yes" | "YES" => return true,
            "n" | "N" | "No" | "no" | "NO" => return false,
            _ => println!("invalid answer")
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entry {
    update_timestamp: u64,
    entry_id: String,
    invite: discord::Invite
}

impl From<discord::Invite> for Entry {
    fn from(invite: discord::Invite) -> Entry {
        Entry {
            update_timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            entry_id: invite.code.clone(),
            invite
        }
    }
}

impl std::cmp::PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        other.entry_id == self.entry_id
    }
}

impl std::cmp::Eq for Entry {

}

impl std::cmp::PartialOrd for Entry {
    fn partial_cmp(&self, other: &Entry) -> std::option::Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::cmp::Ord for Entry {
    fn cmp(&self, other: &Entry) -> std::cmp::Ordering {
        if self.entry_id == other.entry_id && self.update_timestamp == other.update_timestamp {
            std::cmp::Ordering::Equal
        } else if self.entry_id < other.entry_id {
            std::cmp::Ordering::Less
        } else if self.entry_id > other.entry_id {
            std::cmp::Ordering::Greater
        } else if self.update_timestamp > other.update_timestamp {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    }
}

impl Document for Entry {
    type UIDType = String;

    fn get_uid(&self) -> &String {
        &self.invite.code
    }
}

fn main() {
    let matches = clap_app!(discord_guild_crawler =>
        (version: format!("{}.{}.{}", MAJOR, MINOR, PATCH).as_str())
        (author: "Mubelotix <mubelotix@gmail.com>")
        (about: "Crawler for Discord's guild invite links.")
        (@arg HOST: -h --host +takes_value default_value("http://localhost:7700") "MeiliSearch server root url")
        (@arg INDEX: -i --index +takes_value default_value("discord-guilds") "MeiliSearch index name")
        (@arg KEY: -k --key +takes_value "MeiliSearch server key (requires write access)")
    ).get_matches();

    let host = matches.value_of("HOST").unwrap_or_else(|| "http://localhost:7700");
    let key = matches.value_of("KEY").unwrap_or_else(|| "");
    let index = matches.value_of("INDEX").unwrap_or_else(|| "discord-guilds");

    loop {
        let start_timestamp = SystemTime::now();
        let mut guilds: Vec<Entry> = Vec::new();

        match File::open("guilds.cbor") {
            Ok(file) => {
                match serde_cbor::from_reader(&file) {
                    Ok(saved_guilds) => {
                        guilds = saved_guilds;
                    },
                    Err(e) => {
                        eprint!("ERROR: Failed to deserialize data: {:?}.\nSTATUS: Corrupted data may be lost. ", e);
                        if ask("Do you want to continue and overwrite the saved data? (disrecommended)") {
                            println!("Data will be overwritten.");
                        } else {
                            println!("To fix the problem, visit the Github repos. Feel free to ask for help.");
                            std::process::exit(74);
                        };
                    },
                }
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                eprint!("ERROR: Failed to open file: {:?}.\nSTATUS: Potential data loss. ", e);
                if ask("Do you want to continue and ignore the saved data (may be overwritten) ? (disrecommended)") {
                    println!("Ignoring data.");
                } else {
                    println!("To fix the problem, visit the Github repos after trying to launch the program again. Feel free to ask for help.");
                    std::process::exit(74);
                };
            }
            Err(_e) => {
                eprintln!("ERROR: Database file not found.\nSTATUS: It will be created later.");
            }
        }

        let mut links: Vec<String> = Vec::new();
        for page in 0..20 {
            sleep(Duration::from_secs(10));

            println!("google search");
            match google::search(page) {
                Ok(mut new_links) if !new_links.is_empty() => {
                    println!("{} results", new_links.len());
                    links.append(&mut new_links);
                },
                Ok(_empty) => {
                    break;
                },
                Err(e) => {
                    eprintln!("Error: Failed to load links {:?}", e);
                }
            };
        }
        
        let mut loaded_links = Vec::new();
        let mut progress_bar = ProgressBar::new(links.len());
        for link in links {
            progress_bar.set_action("Waiting", Color::Yellow, Style::Normal);
            sleep(Duration::from_secs(6));
            progress_bar.set_action("Loading", Color::Blue, Style::Bold);

            if let Ok(links) = intermediary::resolve(&link) {
                let mut requires_cooldown = false;
                for invite_link in links {
                    if !loaded_links.contains(&invite_link) {
                        if requires_cooldown {
                            progress_bar.set_action("Waiting", Color::Yellow, Style::Normal);
                            sleep(Duration::from_secs(6));
                        }
                        progress_bar.set_action("Verifying", Color::Green, Style::Normal);
                        
                        if let Ok(invite) = discord::Invite::fetch(&invite_link) {
                            progress_bar.print_info("Found", &format!("invite to {}: {}", &invite.guild.as_ref().map(|g| &g.name).unwrap_or(&"Unknown".to_string()), invite.get_url()), Color::Green, Style::Bold);
                            guilds.push(Entry::from(invite));
                        }
                        requires_cooldown = true;
                        loaded_links.push(invite_link);
                    }
                }
            }

            progress_bar.inc();
        }
        progress_bar.finalize();
        
        guilds.sort();
        guilds.dedup();

        let mut success = false;
        while !success {
            if let Ok(file) = File::create("guilds.cbor") {
                match serde_cbor::to_writer(file, &guilds) {
                    Ok(()) => success = true,
                    Err(e) => {
                        eprintln!("ERROR: Failed to save data: {:?}", e);
                    }
                }
            }
            if !success {
                eprintln!("Failed to save database. Press Enter to retry.");
                std::io::stdin().read_line(&mut String::new()).unwrap();
            }
        }
        
        let client = Client::new(host, key);
        match client.get_or_create(index) {
            Ok(mut index) => match index.delete_all_documents() {
                Ok(_progress) => match index.add_documents(guilds, Some("entry_id")) {
                    Ok(_progress) => {
                        // success
                    },
                    Err(e) => {
                        eprintln!("ERROR: Failed to add documents to the index: {:?}\nSTATUS: Index is empty.", e);
                    }
                }
                Err(e) => {
                    eprintln!("ERROR: Failed to remove outdated documents of the index: {:?}\nSTATUS: Index is outdated.", e);
                }
            },
            Err(e) => {
                eprintln!("ERROR: Failed to get or create the index: {:?}\nSTATUS: Index is out of control.", e);
            }
        }
        
        sleep(Duration::from_secs(3600u64.saturating_sub(SystemTime::now().duration_since(start_timestamp).unwrap().as_secs())))
    }
}