use discord_finder::*;
use clap::clap_app;
use pkg_version::*;
use serde::{Serialize, Deserialize};
use std::{thread::sleep, time::{Duration, SystemTime, UNIX_EPOCH}, io::prelude::*, fs::File};
use meilisearch_sdk::{client::Client, document::Document};
use progress_bar::{progress_bar::ProgressBar, color::*};

const MAJOR: u32 = pkg_version_major!();
const MINOR: u32 = pkg_version_minor!();
const PATCH: u32 = pkg_version_patch!();

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

        if let Ok(mut file) = File::open("guilds.json") {
            let mut content = Vec::new();
            if let Ok(_t) = file.read_to_end(&mut content) {
                if let Ok(saved_guilds) = bincode::deserialize(&content) {
                    guilds = saved_guilds;
                }
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
        
        let mut progress_bar = ProgressBar::new(links.len());
        for link in links {
            progress_bar.set_action("Waiting", Color::Yellow, Style::Normal);
            sleep(Duration::from_secs(6));
            progress_bar.set_action("Loading", Color::Blue, Style::Bold);

            progress_bar.set_action("Verifying", Color::Green, Style::Normal);
            if let Ok(links) = intermediary::resolve(&link) {
                for invite_link in links {
                    if let Ok(invite) = discord::Invite::fetch(&invite_link) {
                        progress_bar.print_info("Found", &format!("invite to {}: {}", &invite.guild.as_ref().map(|g| &g.name).unwrap_or(&"Unknown".to_string()), invite.get_url()), Color::Green, Style::Bold);
                        guilds.push(Entry::from(invite));
                    }
                }
            }

            progress_bar.inc();
        }
        progress_bar.finalize();
        
        guilds.sort();
        guilds.dedup();

        if let Ok(mut file) = File::create("guilds.json") {
            if let Ok(data) = bincode::serialize(&guilds) {
                if let Ok(()) = file.write_all(&data) {

                }
            }
        }
        
        let client = Client::new(host, key);
        let mut index = client.get_or_create(index).unwrap();
        index.add_documents(guilds, Some("entry_id")).unwrap();
        
        sleep(Duration::from_secs(3600u64.saturating_sub(SystemTime::now().duration_since(start_timestamp).unwrap().as_secs())))
    }
}
