use reqwest;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::io::Cursor;
use ssh_key::{PrivateKey, PublicKey};
use tokio::sync::mpsc::UnboundedSender;
use crate::AppMessage;

pub async fn start_listener(topic: &str) {
    println!("👂 IPFS Pubsub arka planda dinleniyor: {}", topic);

    use tokio::process::Command as TokioCommand;
    use std::process::{Command as StdCommand, Stdio};
    use tokio::io::{AsyncBufReadExt, BufReader};
    use ssh_key::PublicKey;
    use serde_json::json;

    let topic_clone = topic.to_string();

    // 1. AĞA İLK KATILIŞTA SENKRONİZASYON İSTEĞİ (SYNC_REQ) FIRLAT
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await; // Dinleyicinin tamamen açılmasını bekle
        println!("📡 Ağdaki diğer Peer'lardan güncel oyun listesi isteniyor...");
        let req_payload = json!({"action": "SYNC_REQ"}).to_string() + "\n";
        
        let mut child = TokioCommand::new("ipfs")
            .arg("pubsub").arg("pub").arg(&topic_clone)
            .stdin(Stdio::piped()).spawn().unwrap();
            
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(req_payload.as_bytes()).await;
        }
    });

    // 2. ANA DİNLEME DÖNGÜSÜ
    loop {
        println!("🔄 [SİSTEM] IPFS CLI üzerinden frekansa bağlanılıyor...");
        
        let mut child = TokioCommand::new("ipfs")
            .arg("pubsub").arg("sub").arg(topic)
            .stdout(Stdio::piped())
            .kill_on_drop(true) 
            .spawn()
            .expect("❌ IPFS CLI başlatılamadı");

        println!("✅ [SİSTEM] Kanal açıldı. Frekans dinleniyor...");

        if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            
            while let Ok(bytes_read) = reader.read_line(&mut line).await {
                if bytes_read == 0 { break; } 
                
                let trimmed = line.trim();
                if trimmed.is_empty() { 
                    line.clear();
                    continue; 
                }
                
                // GELEN HAM JSON'I AYRIŞTIR
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    
                    // DURUM 1: BİRİSİ AĞA YENİ GİRDİ VE LİSTE İSTİYOR
                    if json_val.get("action").and_then(|v| v.as_str()) == Some("SYNC_REQ") {
                        let mut registry_path = dirs::home_dir().unwrap();
                        registry_path.push(".fairplay"); registry_path.push("registry.json");

                        // Eğer kendi kütüphanemiz boş değilse ona gönderelim
                        if registry_path.exists() && fs::metadata(&registry_path).map(|m| m.len()).unwrap_or(0) > 5 {
                            println!("🤝 Ağdan senkronizasyon isteği geldi. Yerel liste paylaşılıyor...");
                            if let Ok(add_output) = StdCommand::new("ipfs").arg("add").arg("-Q").arg(&registry_path).output() {
                                let cid = String::from_utf8_lossy(&add_output.stdout).trim().to_string();
                                let res_payload = json!({"action": "SYNC_RES", "cid": cid}).to_string() + "\n";

                                let mut pub_child = StdCommand::new("ipfs").arg("pubsub").arg("pub").arg(topic).stdin(Stdio::piped()).spawn().unwrap();
                                if let Some(mut stdin) = pub_child.stdin.take() {
                                    use std::io::Write;
                                    let _ = stdin.write_all(res_payload.as_bytes());
                                }
                            }
                        }
                        line.clear();
                        continue;
                    }

                    // DURUM 2: AĞDAN GÜNCEL KÜTÜPHANE CEVABI GELDİ
                    if json_val.get("action").and_then(|v| v.as_str()) == Some("SYNC_RES") {
                        if let Some(cid) = json_val.get("cid").and_then(|v| v.as_str()) {
                            println!("📥 Ağdan toplu kütüphane CID'si alındı: {}. İşleniyor...", cid);
                            if let Ok(cat_output) = StdCommand::new("ipfs").arg("cat").arg(cid).output() {
                                let downloaded_json = String::from_utf8_lossy(&cat_output.stdout);
                                
                                if let Ok(remote_games) = serde_json::from_str::<Vec<crate::registry::Game>>(&downloaded_json) {
                                    let mut added = 0;
                                    for game in remote_games {
                                        // Zaten var olan veya geçersiz imzalı olanları save_game otomatik filtreler
                                        if crate::registry::save_game(game).is_ok() { 
                                            added += 1; 
                                        }
                                    }
                                    if added > 0 {
                                        println!("✅ Stateful Sync tamamlandı! {} yeni/güncel oyun eklendi.", added);
                                    } else {
                                        println!("⚡ Gelen liste zaten mevcut kütüphane ile aynı. Ekstra işlem yapılmadı.");
                                    }
                                }
                            }
                        }
                        line.clear();
                        continue;
                    }

                    // DURUM 3: NORMAL TEKİL OYUN DUYURUSU (Mevcut Mantık)
                    if let Ok(oyun) = serde_json::from_str::<crate::registry::Game>(trimmed) {
                        let cert_payload = format!("{}:{}", oyun.id, oyun.developer_pubkey);
                        let master_key_res = PublicKey::from_openssh(crate::registry::MASTER_PUBKEY);
                        
                        if let Ok(master_key) = master_key_res {
                            if let Ok(platform_sig) = oyun.platform_certificate.parse::<ssh_key::SshSig>() {
                                if master_key.verify("fairplay-namespace", cert_payload.as_bytes(), &platform_sig).is_err() {
                                    eprintln!("\n🚨 [REDDEDİLDİ] '{}' için sahte geliştirici anahtarı tespit edildi!", oyun.name);
                                    line.clear(); continue;
                                }
                            } else {
                                line.clear(); continue;
                            }
                        }

                        let payload_to_verify = format!("{}:{}:{}:{}", oyun.id, oyun.name, oyun.cid, oyun.version);
                        let is_valid = PublicKey::from_openssh(&oyun.developer_pubkey)
                            .and_then(|pub_key| {
                                let signature = oyun.signature.parse::<ssh_key::SshSig>().map_err(|_| ssh_key::Error::Crypto)?;
                                pub_key.verify("fairplay-namespace", payload_to_verify.as_bytes(), &signature)
                            });

                        if is_valid.is_ok() {
                            println!("\n🔐 [GÜVENLİ] İmza doğrulandı!");
                            if let Err(e) = crate::registry::save_game(oyun.clone()) {
                                eprintln!("❌ Kayıt Hatası: {}", e);
                            } else {
                                println!("🎉 [BAŞARI] YENİ OYUN DUYULDU: {} (v{})", oyun.name, oyun.version);
                            }
                        }
                    }
                }
                line.clear(); 
            }
        }
        
        let _ = child.wait().await;
        println!("⚠️ [SİSTEM] IPFS bağlantısı koptu, 2 saniye sonra yeniden bağlanılacak...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

pub async fn publish_game(
    name: &str, 
    file_path: &str, 
    channel: &str, 
    executable: Option<String>,
    existing_id: Option<String>,
    unpublish: bool,
    version: u32,
    private_key_path: Option<String>,
    cert: String
) -> Result<(), Box<dyn std::error::Error>> {
    
    let key_path = private_key_path.unwrap_or_else(|| {
        let mut p = dirs::home_dir().unwrap();
        p.push(".ssh"); p.push("id_ed25519");
        p.to_string_lossy().to_string()
    });
    
    let private_key = PrivateKey::read_openssh_file(std::path::Path::new(&key_path))
        .map_err(|_| format!("SSH Özel Anahtarı bulunamadı: {}. Lütfen 'ssh-keygen -t ed25519' ile bir anahtar oluşturun.", key_path))?;
    let pub_key_str = private_key.public_key().to_openssh()?;

    let cid = if unpublish {
        println!("🗑️ Yayından kaldırma anonsu hazırlanıyor...");
        "NULL".to_string()
    } else {
        println!("📦 Klasör/Dosya IPFS ağına yükleniyor...");
        let add_output = std::process::Command::new("ipfs")
            .arg("add").arg("-r").arg("-Q").arg(file_path).output()?;

        if !add_output.status.success() {
            return Err(format!("IPFS Ekleme Hatası: {}", String::from_utf8_lossy(&add_output.stderr)).into());
        }
        let generated_cid = String::from_utf8_lossy(&add_output.stdout).trim().to_string();
        println!("✅ Dosyalar başarıyla eklendi! Yeni CID: {}", generated_cid);
        generated_cid
    };

    let id = existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let payload_to_sign = format!("{}:{}:{}:{}", id, name, cid, version);
    let signature = private_key.sign("fairplay-namespace", ssh_key::HashAlg::Sha256, payload_to_sign.as_bytes())?;
    let signature_str = signature.to_string(); 
    
    let game = crate::registry::Game {
        id: id.clone(),
        name: name.to_string(),
        cid: cid.clone(),
        timestamp: Utc::now(),
        executable,
        version,
        developer_pubkey: pub_key_str,
        platform_certificate: cert,
        signature: signature_str,
    };
    
    let mut game_json = serde_json::to_string(&game)?;
    game_json.push('\n'); 
    println!("📢 Anons '{}' kanalındaki Peer'lara duyuruluyor...", channel);
    
    let mut child = std::process::Command::new("ipfs")
        .arg("pubsub").arg("pub").arg(channel)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(game_json.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        if unpublish {
            println!("🎉 '{}' isimli oyunu kaldırma isteği ağa başarıyla iletildi!", name);
        } else {
            println!("🎉 Oyun (v{}) ağa başarıyla imzalanıp yayınlandı! (ID: {})", version, id);
        }
        
        if let Err(e) = crate::registry::save_game(game) {
            eprintln!("{}", e);
        }
    } else {
        let err_text = String::from_utf8_lossy(&output.stderr);
        eprintln!("❌ Ağa duyururken CLI hatası oluştu: {}", err_text);
    }

    Ok(())
}

pub async fn fetch_game(id: &str, tx: Option<UnboundedSender<AppMessage>>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let games = crate::registry::load_games();
    let game = games.iter().find(|g| g.id == id).ok_or("Kayıtlarda bu ID ile oyun bulunamadı!")?;
    let cid = &game.cid;

    let mut cache_dir = dirs::home_dir().ok_or("HOME dizini bulunamadı")?;
    cache_dir.push(".fairplay"); cache_dir.push("cache"); cache_dir.push(id); 

    if cache_dir.exists() { return Ok(cache_dir); }

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:5001/api/v0/get?arg={}", cid);
    let mut response = client.post(&url).send().await?;
    
    if !response.status().is_success() { return Err("IPFS indirme hatası".into()); }

    let total_size = response.content_length();
    let mut downloaded: u64 = 0;
    let mut bytes = Vec::new();

    while let Some(chunk) = response.chunk().await? {
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        
        if let Some(sender) = &tx {
            let progress = total_size.map(|t| (downloaded as f32) / (t as f32)).unwrap_or(-1.0);
            let _ = sender.send(AppMessage::DownloadProgress { 
                game_id: id.to_string(), 
                progress,
                downloaded
            });
        }
    }

    fs::create_dir_all(&cache_dir)?;
    let cursor = Cursor::new(bytes);
    let mut archive = tar::Archive::new(cursor);
    archive.unpack(&cache_dir)?;

    Ok(cache_dir)
}

pub fn launch_game(game: &crate::registry::Game) -> Result<(), Box<dyn std::error::Error>> {
    let mut cache_path = dirs::home_dir().ok_or("HOME dizini bulunamadı")?;
    cache_path.push(".fairplay"); cache_path.push("cache"); cache_path.push(&game.id);

    if let Some(exec_path) = &game.executable {
        let mut full_exec_path = cache_path.join(exec_path);

        if !full_exec_path.exists() {
            let file_name = std::path::Path::new(exec_path).file_name().unwrap();
            let alt_path_1 = cache_path.join(&game.cid).join(exec_path);
            let alt_path_2 = cache_path.join(&game.cid).join(file_name);
            let alt_path_3 = cache_path.join(file_name);

            if alt_path_1.exists() { full_exec_path = alt_path_1; } 
            else if alt_path_2.exists() { full_exec_path = alt_path_2; } 
            else if alt_path_3.exists() { full_exec_path = alt_path_3; }
        }

        if !full_exec_path.exists() {
            return Err(format!("Çalıştırılabilir dosya bulunamadı: {}", exec_path).into());
        }

        let work_dir = full_exec_path.parent().unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&full_exec_path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&full_exec_path, perms);
            }
        }

        let mut child = std::process::Command::new(&full_exec_path)
            .current_dir(work_dir)
            .spawn()?;
        
        child.wait()?;
        Ok(())
    } else {
        Err("Bu oyun için otomatik başlatma verisi (executable) tanımlanmamış.".into())
    }
}