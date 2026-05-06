use reqwest;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::io::{Cursor, Write};
use std::process::{Command, Stdio};
use indicatif::{ProgressBar, ProgressStyle};
use ssh_key::{PrivateKey};



pub async fn start_listener(topic: &str) {
    println!("👂 IPFS Pubsub arka planda dinleniyor: {}", topic);

    use std::process::{Command, Stdio};
    use std::io::{BufRead, BufReader};
    use ssh_key::PublicKey;

    loop {
        println!("🔄 [SİSTEM] IPFS CLI üzerinden frekansa bağlanılıyor...");
        
        let mut child = Command::new("ipfs")
            .arg("pubsub").arg("sub").arg(topic)
            .stdout(Stdio::piped())
            .spawn()
            .expect("❌ IPFS CLI başlatılamadı");

        println!("✅ [SİSTEM] Kanal açıldı. Frekans dinleniyor...");

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            
            for line_result in reader.lines() {
                if let Ok(line) = line_result {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    
                    // IPFS CLI doğrudan ham JSON string'ini ekrana basar! Base64 çözmeye gerek yok.
                    if let Ok(oyun) = serde_json::from_str::<crate::registry::Game>(line) {
                        // 1. AŞAMA: PLATFORM SERTİFİKASI DOĞRULAMASI
                        let cert_payload = format!("{}:{}", oyun.id, oyun.developer_pubkey);
                        let master_key_res = PublicKey::from_openssh(crate::registry::MASTER_PUBKEY);
                        
                        if let Ok(master_key) = master_key_res {
                            if let Ok(platform_sig) = oyun.platform_certificate.parse::<ssh_key::SshSig>() {
                                if master_key.verify("fairplay-namespace", cert_payload.as_bytes(), &platform_sig).is_err() {
                                    eprintln!("\n🚨 [REDDEDİLDİ] '{}' için sahte geliştirici anahtarı tespit edildi! (Sertifika Geçersiz)", oyun.name);
                                    continue;
                                }
                            } else {
                                eprintln!("\n🚨 [REDDEDİLDİ] '{}' oyununun sertifika formatı bozuk!", oyun.name);
                                continue;
                            }
                        } else {
                            eprintln!("\n⚠️ [SİSTEM HATASI] MASTER_PUBKEY geçersiz! Lütfen registry.rs dosyasını güncelleyin.");
                            continue;
                        }

                        // 2. AŞAMA: GELİŞTİRİCİ İMZASI DOĞRULAMASI
                        let payload_to_verify = format!("{}:{}:{}:{}", oyun.id, oyun.name, oyun.cid, oyun.version);
                        
                        let is_valid = PublicKey::from_openssh(&oyun.developer_pubkey)
                            .and_then(|pub_key| {
                                let signature = oyun.signature.parse::<ssh_key::SshSig>().map_err(|_| ssh_key::Error::Crypto)?;
                                pub_key.verify("fairplay-namespace", payload_to_verify.as_bytes(), &signature)
                            });

                        if is_valid.is_ok() {
                            println!("\n🔐 [GÜVENLİ] Sertifika ve İmza doğrulandı! Geliştirici yetkili.");
                            if let Err(e) = crate::registry::save_game(oyun.clone()) {
                                eprintln!("❌ Kayıt Hatası: {}", e);
                            } else {
                                println!("🎉 [BAŞARI] OYUN AĞDAN ALINDI: {} (v{})", oyun.name, oyun.version);
                            }
                        } else {
                            eprintln!("\n🚨 [TEHLİKE] {} adlı oyunun içeriği değiştirilmiş! (İmza Geçersiz)", oyun.name);
                        }
                    }
                }
            }
        }
        
        let _ = child.wait();
        println!("⚠️ [SİSTEM] IPFS bağlantısı koptu, 2 saniye sonra yeniden bağlanılacak...");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

// Oyun yayınlama fonksiyonu
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
    
    // SSH Anahtarını Yükle
    let key_path = private_key_path.unwrap_or_else(|| {
        let mut p = dirs::home_dir().unwrap();
        p.push(".ssh"); p.push("id_ed25519");
        p.to_string_lossy().to_string()
    });
    // DÜZELTME 3: &key_path yerine std::path::Path::new(&key_path) kullanıldı
    let private_key = PrivateKey::read_openssh_file(std::path::Path::new(&key_path))
        .map_err(|_| format!("SSH Özel Anahtarı bulunamadı: {}. Lütfen 'ssh-keygen -t ed25519' ile bir anahtar oluşturun.", key_path))?;
    let pub_key_str = private_key.public_key().to_openssh()?;

    let cid = if unpublish {
        println!("🗑️ Yayından kaldırma anonsu hazırlanıyor...");
        "NULL".to_string()
    } else {
        println!("📦 Klasör/Dosya IPFS ağına yükleniyor...");
        let add_output = Command::new("ipfs")
            .arg("add").arg("-r").arg("-Q").arg(file_path).output()?;

        if !add_output.status.success() {
            return Err(format!("IPFS Ekleme Hatası: {}", String::from_utf8_lossy(&add_output.stderr)).into());
        }
        let generated_cid = String::from_utf8_lossy(&add_output.stdout).trim().to_string();
        println!("✅ Dosyalar başarıyla eklendi! Yeni CID: {}", generated_cid);
        generated_cid
    };

    // ID BELİRLEME: Güncelleme ise var olanı kullan, ilk yükleme ise yeni UUID üret
    let id = existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // İMZA OLUŞTURMA: ID + Name + CID + Version
    let payload_to_sign = format!("{}:{}:{}:{}", id, name, cid, version);
    let signature = private_key.sign("fairplay-namespace", ssh_key::HashAlg::Sha256, payload_to_sign.as_bytes())?;
    let signature_str = signature.to_string(); // SshSig to string
    
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
    
    let mut child = Command::new("ipfs")
        .arg("pubsub").arg("pub").arg(channel)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(game_json.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if output.status.success() {
        if unpublish {
            println!("🎉 '{}' isimli oyunu kaldırma isteği ağa başarıyla iletildi!", name);
        } else {
            println!("🎉 Oyun (v{}) ağa başarıyla imzalanıp yayınlandı! (ID: {})", version, id);
        }
        
        // Kendi anonsumuzu yerel registry'ye kaydedelim
        if let Err(e) = crate::registry::save_game(game) {
            eprintln!("{}", e);
        }
    } else {
        let err_text = String::from_utf8_lossy(&output.stderr);
        eprintln!("❌ Ağa duyururken CLI hatası oluştu: {}", err_text);
    }

    Ok(())
}

pub async fn fetch_game(id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let games = crate::registry::load_games();
    let game = games.iter().find(|g| g.id == id).ok_or("Kayıtlarda bu ID ile oyun bulunamadı!")?;
    let cid = &game.cid;

    let mut cache_dir = dirs::home_dir().ok_or("HOME dizini bulunamadı")?;
    cache_dir.push(".fairplay");
    cache_dir.push("cache");
    cache_dir.push(id); 

    if cache_dir.exists() {
        println!("⚡ Oyun zaten önbellekte (cache) mevcut. İndirme atlanıyor...");
        return Ok(cache_dir); 
    }

    println!("📥 Oyun IPFS ağından indiriliyor (CID: {})...", cid);

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:5001/api/v0/get?arg={}", cid);
    
    let response = client.post(&url).send().await?;
    
    if !response.status().is_success() {
        return Err(format!("IPFS indirme hatası: Çıktı kodu {}", response.status()).into());
    }

    let total_size = response.content_length();
    let pb = match total_size {
        Some(size) => ProgressBar::new(size), 
        None => ProgressBar::new_spinner(),   
    };

    let style = if total_size.is_some() {
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-")
    } else {
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] 📥 İndirilen Veri: {bytes} ({bytes_per_sec})")
            .unwrap()
    };
    pb.set_style(style);

    let mut bytes = Vec::new();
    let mut mut_response = response;

    while let Some(chunk) = mut_response.chunk().await? {
        pb.inc(chunk.len() as u64);       
        bytes.extend_from_slice(&chunk);  
    }
    pb.finish_with_message("✅ İndirme tamamlandı!");

    println!("📦 Arşiv çıkartılıyor (Extract) -> {:?}", cache_dir);
    fs::create_dir_all(&cache_dir)?;
    
    let cursor = Cursor::new(bytes);
    let mut archive = tar::Archive::new(cursor);
    archive.unpack(&cache_dir)?;

    println!("✅ Oyun başarıyla çıkartıldı ve diskte hazır!");

    Ok(cache_dir)
}
