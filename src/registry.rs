use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Game {
	pub id: String,
	pub name: String,
	pub cid: String,
	pub timestamp: DateTime<Utc>,
}

fn get_registry_path() -> PathBuf {
    let mut path = dirs::home_dir().expect("HOME is not found");
    path.push(".fairplay");
    
    // Klasör yoksa oluştur
    if !path.exists() {
        fs::create_dir_all(&path).expect("~/.fairplay couldnt created");
    }
    
    path.push("registry.json");
    path
}

pub fn init() {
	let path = get_registry_path();
        if !path.exists() {
        	let mut file = File::create(&path).expect("Couldn't create registery.json");
        	file.write_all(b"[]").expect("Couldn't write entry data");
        	println!("Created new registery file: {:?}", path);
        }
}

pub fn load_games() -> Vec<Game> {
    let path = get_registry_path();
    if !path.exists() {
        return Vec::new();
    }
    
    let data = fs::read_to_string(&path).expect("Registry okunamadi");
    serde_json::from_str(&data).unwrap_or_else(|_| Vec::new())
}

// Task 3.4 (B): Yeni Oyun Ekleme (Save)
pub fn save_game(game: Game) {
    let mut games = load_games();
    games.push(game);
    
    let path = get_registry_path();
    // JSON'u okunaklı (pretty) formatta kaydet
    let data = serde_json::to_string_pretty(&games).expect("JSON serilestirilemedi");
    fs::write(path, data).expect("Registry dosyasina yazilamadi");
}
