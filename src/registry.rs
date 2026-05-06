use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

// PLATFORM KÖK ANAHTARI (Hardcoded Root of Trust)
// DİKKAT: Buradaki değeri kendi `cat ~/.ssh/id_ed25519.pub` çıktınla değiştirmeyi unutma!
pub const MASTER_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHXZNPx5PmisWZxvbMFQGsHnqRpBNve/6nKR9LQGN1o/ fairplay-admin";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub cid: String,
    pub timestamp: DateTime<Utc>,
    pub executable: Option<String>,
    pub version: u32,
    pub developer_pubkey: String,
    pub platform_certificate: String,
    pub signature: String,
}

fn get_registry_path() -> PathBuf {
    let mut path = dirs::home_dir().expect("Home dizini bulunamadı");
    path.push(".fairplay");
    path.push("registry.json");
    path
}

pub fn init() {
    let path = get_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("Fairplay dizini oluşturulamadı");
    }
    if !path.exists() {
        let mut file = File::create(&path).expect("Registry dosyası oluşturulamadı");
        file.write_all(b"[]").expect("Boş JSON yazılamadı");
    }
}

pub fn load_games() -> Vec<Game> {
    let path = get_registry_path();
    if !path.exists() {
        return Vec::new();
    }
    let data = fs::read_to_string(path).expect("Registry okunamadı");
    serde_json::from_str(&data).unwrap_or_else(|_| Vec::new())
}

pub fn save_game(new_game: Game) -> Result<(), String> {
    let mut games = load_games();

    if new_game.cid == "NULL" {
        games.retain(|g| g.id != new_game.id);
        println!("🗑️ Oyun (ID: {}) geliştiricisi tarafından yayından kaldırıldı.", new_game.id);
    } else {
        if let Some(existing_game) = games.iter_mut().find(|g| g.id == new_game.id) {
            
            // KURAL 1: APP SIGNING KEY PINNING
            if existing_game.developer_pubkey != new_game.developer_pubkey {
                return Err(format!("🚨 GÜVENLİK İHLALİ: '{}' için sahte güncelleme! İmzalar eşleşmiyor.", new_game.name));
            }

            // KURAL 2: DOWNGRADE PROTECTION
            if new_game.version < existing_game.version {
                return Err(format!("⚠️ SÜRÜM DÜŞÜRME ENGELLENDİ: Gelen sürüm (v{}) mevcut sürümden (v{}) daha eski.", new_game.version, existing_game.version));
            }

            // GÜNCELLEME: Rust'ın dereference özelliğini kullanarak tüm yapıyı tek seferde güvenle güncelliyoruz
            if new_game.version > existing_game.version || new_game.timestamp > existing_game.timestamp {
                *existing_game = new_game; 
            }
        } else {
            games.push(new_game);
        }
    }

    let path = get_registry_path();
    let data = serde_json::to_string_pretty(&games).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())?;
    
    Ok(())
}

pub fn is_game_in_cache(id: &str) -> bool {
    if let Some(mut path) = dirs::home_dir() {
        path.push(".fairplay");
        path.push("cache");
        path.push(id);
        return path.exists();
    }
    false
}