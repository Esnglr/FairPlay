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
    let client = reqwest::Client::new();
    let url = "http://127.0.0.1:5001/api/v0/pubsub/sub?arg=fairplay-games";

    loop {
        let res = client.post(url).send().await;
        match res {
            Ok(mut response) => {
                let mut buffer = String::new(); // Parçalı chunk'ları toplamak için

                while let Ok(Some(chunk)) = response.chunk().await {
                    if let Ok(text) = String::from_utf8(chunk.to_vec()) {
                        buffer.push_str(&text);

                        // Tam bir satır (\n) gelene kadar döngüde kal
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].trim().to_string();
                            buffer = buffer[pos + 1..].to_string(); // İşlenen kısmı at

                            if line.is_empty() { continue; }

                            // Debug: Gelen ham satırı gör
                            // println!("DEBUG - Gelen Ham Satır: {}", line);

                            match serde_json::from_str::<IpfsZarf>(&line) {
                                Ok(zarf) => {
                                    if let Some(base64_data) = zarf.data {
                                        process_incoming_data(base64_data).await;
                                    } else {
                                        eprintln!("⚠️ Zarf geldi ama 'data' alanı boş!");
                                    }
                                }
                                Err(e) => eprintln!("❌ Parse Hatası (JSON tam gelmemiş olabilir): {}", e),
                            }
                        }
                    }
                }
            }
            Err(e) => eprintln!("Bağlantı hatası: {}. 2sn içinde tekrar denenecek...", e),
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

// Yardımcı fonksiyon: Decode ve Kayıt
async fn process_incoming_data(base64_data: String) {
    // IPFS bazen standart bazen URL_SAFE base64 gönderir
    let decoded = general_purpose::STANDARD.decode(&base64_data)
        .or_else(|_| general_purpose::URL_SAFE.decode(&base64_data))
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(&base64_data));

    match decoded {
        Ok(bytes) => {
            if let Ok(oyun) = serde_json::from_slice::<registry::Game>(&bytes) {
                println!("\n🎉 [P2P] Yeni oyun: {} (CID: {})", oyun.name, oyun.cid);
                registry::save_game(oyun); // registry.json'a kaydet[cite: 3]
            }
        }
        Err(e) => eprintln!("❌ Decode başarısız: {}", e),
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