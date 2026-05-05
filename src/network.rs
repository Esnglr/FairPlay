use reqwest;
use base64::{Engine as _, engine::general_purpose};
use serde::Deserialize;
use crate::registry;
use std::fs::File;
use std::io::Read;
use chrono::Utc;

#[derive(Deserialize)]
struct IpfsZarf {
    data: Option<String>,
}

#[derive(serde::Deserialize)]
struct IpfsAddResponse {
    #[serde(rename = "Hash")] // Bize sadece Hash (CID) lazım
    hash: String,
}

pub async fn start_listener() {
    println!("👂 IPFS Pubsub arka planda dinleniyor...");

    let client = reqwest::Client::new();
    let url = "http://127.0.0.1:5001/api/v0/pubsub/sub?arg=uZmFpcnBsYXktZ2FtZXM";
    loop {
        println!("🔄 [SİSTEM] IPFS API'sine bağlanılıyor...");
        match client.post(url).send().await {
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
pub async fn publish_game(name: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();

    // ADIM 1: Dosyayı oku ve Multipart Form oluştur (Task 5.1)
    let mut file = File::open(file_path).expect("Dosya bulunamadı! (Lütfen tam yolu girin)");
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Dosyayı form verisi olarak paketle
    let part = reqwest::multipart::Part::bytes(buffer).file_name(file_path.to_string());
    let form = reqwest::multipart::Form::new().part("file", part);

    println!("📦 Dosya IPFS ağına yükleniyor...");
    let add_res = client.post("http://127.0.0.1:5001/api/v0/add")
        .multipart(form)
        .send()
        .await?;

    // ADIM 2: Dönen JSON'dan Hash (CID) değerini ayıkla (Task 5.2)
    let add_text = add_res.text().await?;
    let ipfs_data: IpfsAddResponse = serde_json::from_str(&add_text)
        .expect("IPFS yanıtı parse edilemedi");
    let cid = ipfs_data.hash;
    println!("✅ Dosya başarıyla eklendi! CID: {}", cid);

    // ADIM 3: Oyun objesini (JSON) oluştur (Task 5.3)
    let game = crate::registry::Game {
        id: cid.clone(), // Benzersiz ID olarak oyunun kendi Hash'ini kullanıyoruz
        name: name.to_string(),
        cid: cid.clone(),
        timestamp: Utc::now(),
    };
    
    // Objemizi ağa yollamak üzere String'e çeviriyoruz
    let game_json = serde_json::to_string(&game)?;

    // ADIM 4: PubSub üzerinden ağa duyur (Task 5.4) - SHELL (CLI) HACK
    println!("📢 Oyun ağdaki diğer Peer'lara (Gossip) duyuruluyor...");

    use std::process::{Command, Stdio};
    use std::io::Write;

    // IPFS sürecini (process) başlatıyoruz ama çalıştırmayı bekletip STDIN borusu açıyoruz
    let mut child = Command::new("ipfs")
        .arg("pubsub")
        .arg("pub")
        .arg("fairplay-games")
        .stdin(Stdio::piped())  // Veriyi buradan akıtacağız
        .stdout(Stdio::piped()) // Çıktıları yakalamak için
        .stderr(Stdio::piped())
        .spawn()
        .expect("IPFS CLI komutu başlatılamadı (ipfs PATH'te mi?)");

    // Açtığımız borunun (stdin) içine JSON byte'larımızı yazıyoruz
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(game_json.as_bytes()).expect("JSON verisi STDIN'e yazılamadı");
    }

    // Şimdi sürecin bitmesini bekle ve dönen sonucu al
    let output = child.wait_with_output().expect("IPFS CLI süreci beklenemedi");

    if output.status.success() {
        println!("🎉 '{}' isimli oyun başarıyla ağa yayınlandı!", name);
        // Yerel registry'ye (JSON veritabanımıza) kaydet
        crate::registry::save_game(game);
    } else {
        let err_text = String::from_utf8_lossy(&output.stderr);
        eprintln!("❌ Ağa duyururken CLI hatası oluştu: {}", err_text);
    }

    Ok(())
}