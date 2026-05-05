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
    println!("IPFS Pubsub is started at the background");

    let client = reqwest::Client::new();
    let url="http://127.0.0.1:5001/api/v0/pubsub/sub?arg=fairplay-games";

    loop{
        match client.get(url).send().await {
            Ok(mut response) => {
                while let Ok(Some(chunk)) = response.chunk().await {
                    if let Ok(text) = String::from_utf8(chunk.to_vec()){
                        for line in text.lines(){
                            if line.trim().is_empty(){
                                continue;
                            }
                            if let Ok(zarf) = serde_json::from_str::<IpfsZarf>(line){
                                if let Some(base64_data) = zarf.data{
                                    if let Ok(decoded_bytes)= general_purpose::STANDARD.decode(base64_data){
                                        if let Ok(oyun) = serde_json::from_slice::<registry::Game>(&decoded_bytes) {
                                            println!("\n🎉 [P2P] Ağda yeni oyun anons edildi: {} (CID: {})", oyun.name, oyun.cid);
                                            // Veritabanına (JSON dosyasına) kaydet
                                            registry::save_game(oyun);
                                        }     
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(_) => {}
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

    // ADIM 4: PubSub üzerinden ağa duyur (Task 5.4)
    println!("📢 Oyun ağdaki diğer Peer'lara (Gossip) duyuruluyor...");
    
    // IPFS PubSub HTTP API'sinde argümanlar query (URL parametresi) olarak yollanır.
    // Aynı isme ("arg") sahip iki parametre yolluyoruz: 1. Kanal Adı, 2. Mesajın kendisi (Bizim JSON)
    let pub_res = client.post("http://127.0.0.1:5001/api/v0/pubsub/pub")
        .query(&[("arg", "fairplay-games")]) 
        .body(game_json)
        .send()
        .await?;

    if pub_res.status().is_success() {
        println!("🎉 '{}' isimli oyun başarıyla ağa yayınlandı!", name);
        // Duyurduğumuz oyunu kendi yerel listemize de (registry) ekleyelim ki biz de görebilelim
        crate::registry::save_game(game);
    } else {
        println!("❌ Ağa duyururken bir hata oluştu. HTTP Status: {}", pub_res.status());
    }

    Ok(())
}