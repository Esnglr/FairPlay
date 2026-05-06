use reqwest;
use base64::{Engine as _, engine::general_purpose};
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::io::Cursor;
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Deserialize)]
struct IpfsZarf {
    data: Option<String>,
}

#[derive(serde::Deserialize)]
struct IpfsAddResponse {
    #[serde(rename = "Hash")] // Bize sadece Hash (CID) lazım
    hash: String,
}

pub async fn start_listener(topic: &str) {
    println!("👂 IPFS Pubsub arka planda dinleniyor: {}", topic);

    let client = reqwest::Client::new();
    let encoded_topic = multibase::encode(multibase::Base::Base64Url, topic);
    let url = format!("http://127.0.0.1:5001/api/v0/pubsub/sub?arg={}", encoded_topic);
    loop {
        println!("🔄 [SİSTEM] IPFS API'sine bağlanılıyor...");
        match client.post(&url).send().await {
            Ok(mut response) => {
                println!("✅ [SİSTEM] Kanal açıldı. Frekans dinleniyor...");
                let mut buffer: Vec<u8> = Vec::new();

                // Ağdan veri aktığı anda burası tetiklenir
                while let Ok(Some(chunk)) = response.chunk().await {
                    println!("📥 [RÖNTGEN] Ağdan {} byte veri aktı!", chunk.len());
                    buffer.extend_from_slice(&chunk);

                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
                        let line_str = String::from_utf8_lossy(&line_bytes);
                        let line = line_str.trim();

                        if line.is_empty() {
                            continue;
                        }

                        println!("🔍 [RÖNTGEN] Ham veri yakalandı: {}", line);

                        // JSON çözümleme adımları ve özel hata mesajları
                        match serde_json::from_str::<IpfsZarf>(line) {
                            Ok(zarf) => {
                                if let Some(base64_data) = zarf.data {
                                    
                                    let clean_base64 = if base64_data.starts_with('u') {
                                        &base64_data[1..] // Baştaki 'u' harfini at
                                    } else {
                                        &base64_data
                                    };

                                    // Temizlenmiş veriyle decode işlemini yap
                                    let decode_result = general_purpose::URL_SAFE.decode(clean_base64)
                                        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(clean_base64))
                                        .or_else(|_| general_purpose::STANDARD.decode(clean_base64));
                                    
                                    match decode_result {
                                        Ok(decoded_bytes) => {
                                            match serde_json::from_slice::<crate::registry::Game>(&decoded_bytes) {
                                                Ok(oyun) => {
                                                    println!("\n🎉 [BAŞARI] AĞDA YENİ OYUN BULUNDU: {} (CID: {})", oyun.name, oyun.cid);
                                                    crate::registry::save_game(oyun);
                                                }
                                                Err(e) => eprintln!("⚠️ [HATA] Oyun formatı bozuk: {} (Gelen: {:?})", e, String::from_utf8_lossy(&decoded_bytes)),
                                            }
                                        }
                                        Err(e) => eprintln!("⚠️ [HATA] Base64 çözülemedi: {}", e),
                                    }
                                } else {
                                     println!("⚠️ [HATA] Zarf geldi ama içi boş (data alanı yok).");
                                }
                            }
                            Err(e) => eprintln!("⚠️ [HATA] PubSub Zarfı okunamadı: {}", e),
                        }
                    }
                }
                println!("⚠️ [SİSTEM] IPFS API bağlantıyı kopardı, 2 saniye sonra tekrar denenecek...");
            }
            Err(e) => {
                eprintln!("❌ [HATA] IPFS API'sine ulaşılamadı. Hata: {}", e);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

// Oyun yayınlama fonksiyonu
pub async fn publish_game(
    name: &str, 
    file_path: &str, 
    channel: &str, 
    executable: Option<String>,
    existing_id: Option<String>, // YENİ
    unpublish: bool              // YENİ
) -> Result<(), Box<dyn std::error::Error>> {
    
    let cid;
    
    // Eğer unpublish bayrağı verildiyse dosya yükleme, direkt NULL geç!
    if unpublish {
        println!("🗑️ Yayından kaldırma anonsu hazırlanıyor...");
        cid = "NULL".to_string();
    } else {
        println!("📦 Klasör/Dosya IPFS ağına yükleniyor...");
        use std::process::{Command, Stdio};
        
        let add_output = Command::new("ipfs")
            .arg("add").arg("-r").arg("-Q").arg(file_path).output()?;

        if !add_output.status.success() {
            return Err(format!("IPFS Ekleme Hatası: {}", String::from_utf8_lossy(&add_output.stderr)).into());
        }
        cid = String::from_utf8_lossy(&add_output.stdout).trim().to_string();
        println!("✅ Dosyalar başarıyla eklendi! Yeni CID: {}", cid);
    }

    // ID BELİRLEME: Güncelleme ise var olanı kullan, ilk yükleme ise yeni UUID üret
    let id = existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let game = crate::registry::Game {
        id: id.clone(),
        name: name.to_string(),
        cid: cid.clone(),
        timestamp: chrono::Utc::now(),
        executable,
    };
    
    let game_json = serde_json::to_string(&game)?;

    println!("📢 Anons '{}' kanalındaki Peer'lara duyuruluyor...", channel);
    
    use std::process::{Command, Stdio};
    use std::io::Write;
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
            println!("🎉 '{}' isimli oyunun yeni versiyonu başarıyla ağa yayınlandı! (ID: {})", name, id);
        }
        crate::registry::save_game(game);
    } else {
        eprintln!("❌ Ağa duyururken CLI hatası oluştu.");
    }

    Ok(())
}

pub async fn fetch_game(id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Task 7.1: ID'ye karşılık gelen CID'yi registry'den bul
    let games = crate::registry::load_games();
    let game = games.iter().find(|g| g.id == id).ok_or("Kayıtlarda bu ID ile oyun bulunamadı!")?;
    let cid = &game.cid;

    // Task 7.2: Dosyanın ~/.fairplay/cache/<id> dizininde olup olmadığını kontrol et
    let mut cache_dir = dirs::home_dir().ok_or("HOME dizini bulunamadı")?;
    cache_dir.push(".fairplay");
    cache_dir.push("cache");
    cache_dir.push(id); // Her oyunu kendi ID'si ile klasörleyelim

    if cache_dir.exists() {
        println!("⚡ Oyun zaten önbellekte (cache) mevcut. İndirme atlanıyor...");
        return Ok(cache_dir); // Task 9.1'de çalıştırmak üzere bu yolu geri dönüyoruz
    }

    println!("📥 Oyun IPFS ağından indiriliyor (CID: {})...", cid);

    // Task 7.3: IPFS uç noktasına istek atarak dosyayı (.tar) indir
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:5001/api/v0/get?arg={}", cid);
    
    // IPFS RPC uç noktaları HTTP GET kabul etse de resmi standart POST kullanmaktır.
// ... (üst kısımlar aynı)
    let response = client.post(&url).send().await?;
    
    if !response.status().is_success() {
        return Err(format!("IPFS indirme hatası: Çıktı kodu {}", response.status()).into());
    }

    // Task 8.2 & 8.3: İndirme işlemi için Progress Bar ayarları
    let total_size = response.content_length();
    let pb = match total_size {
        Some(size) => ProgressBar::new(size), // Boyut biliniyorsa çubuk yap
        None => ProgressBar::new_spinner(),   // Bilinmiyorsa animasyonlu sayaç yap
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

    // Veriyi tek seferde RAM'e gömmek yerine, Chunk (parça parça) okuyan Stream döngüsü
    let mut bytes = Vec::new();
    let mut mut_response = response;

    while let Some(chunk) = mut_response.chunk().await? {
        pb.inc(chunk.len() as u64);       // 1. Progress Bar'ı gelen parça boyutu kadar ilerlet
        bytes.extend_from_slice(&chunk);  // 2. Parçayı ana buffer'a ekle
    }
    pb.finish_with_message("✅ İndirme tamamlandı!");

    // Task 7.4 (Aynı kaldı): İnen dosya byte'larını aç
    println!("📦 Arşiv çıkartılıyor (Extract) -> {:?}", cache_dir);
    fs::create_dir_all(&cache_dir)?;
    
    let cursor = Cursor::new(bytes);
    let mut archive = tar::Archive::new(cursor);
    archive.unpack(&cache_dir)?;

    println!("✅ Oyun başarıyla çıkartıldı ve diskte hazır!");

    Ok(cache_dir)
}
