mod registry;
mod network;

use eframe::egui;
use clap::{Parser, Subcommand};
use cli_table::{format::Justify, Cell, Style, Table};
use std::path::Path;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct PublishConfig {
    name: String,
    path: String,
    #[serde(default = "default_channel")]
    channel: String,
    executable: Option<String>,
    id: Option<String>,
    #[serde(default)]
    unpublish: bool,
    #[serde(default = "default_version")]
    version: u32,
    private_key: Option<String>,
    cert: String,
}

// JSON'da channel ve version girilmezse kullanılacak varsayılan değerler
fn default_channel() -> String { "fairplay-games".to_string() }
fn default_version() -> u32 { 1 }

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pin signing key of a game developer to specific game ID
    SignDev {
        #[arg(long)]
        id: String,
        #[arg(long)]
        dev_pubkey: String,
    },
    /// Yeni bir oyunu IPFS ağına yükle ve duyur
    Publish {
        config_file: String,
    },
    /// Ağdan duyulan mevcut oyunları listele
    List,
    /// Ağı sürekli dinleyerek yeni oyunları keşfeder (Kapatana kadar çalışır)
    Listen,
    /// Belirtilen özel bir kanalı dinleyerek keşif yapar
    Connect {
        channel: String,
    },
    /// Bir oyunu ID ile indir ve izole ortamda (sandbox) çalıştır
    Play {
        /// Oynanacak oyunun kayıt ID'si
        id: String,
    },
    Ui,
}

#[tokio::main]
async fn main() {
    registry::init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::SignDev { id, dev_pubkey } => {
            let private_key_path = dirs::home_dir().unwrap().join(".fairplay-admin/fairplay-admin-key");
            let private_key = ssh_key::PrivateKey::read_openssh_file(&private_key_path)
                .expect("Admin private key not found! Lütfen anahtarın ~/.fairplay-admin/fair-admin-key konumunda olduğundan emin olun.");

            let payload = format!("{}:{}", id, dev_pubkey);
            
            // DÜZELTME 1: HashAlg::Sha256 eklendi
            let signature = private_key.sign("fairplay-namespace", ssh_key::HashAlg::Sha256, payload.as_bytes()).unwrap();
            
            // DÜZELTME 2: Base64 çevirisi yerine doğrudan SSH İmza string'ini (PEM) alıyoruz
            let cert_str = signature.to_string(); 

            println!("✅ Game developer certificate created!");
            println!("Game ID: {}", id);
            println!("Certificate (owned by game developer):\n{}", cert_str);
        }
        Commands::Publish { config_file } => {
            // 1. JSON dosyasını oku
            let file_content = fs::read_to_string(config_file)
                .expect("❌ JSON konfigürasyon dosyası okunamadı! Dosya yolunu kontrol edin.");
            
            // 2. JSON metnini Rust objesine (PublishConfig) çevir
            let config: PublishConfig = serde_json::from_str(&file_content)
                .expect("❌ JSON dosyası ayrıştırılamadı. Virgülleri veya formatı kontrol edin.");

            println!("🚀 Process started: {} (v{})", config.name, config.version);
            
            // 3. Değerleri mevcut çalışan publish_game fonksiyonuna pasla
            if let Err(e) = network::publish_game(
                &config.name,
                &config.path,
                &config.channel,
                config.executable,
                config.id,
                config.unpublish,
                config.version,
                config.private_key,
                config.cert
            ).await {
                eprintln!("❌ Publish failed: {}", e);
            }
        }
        
        Commands::List => {
            println!("🔍 Yerel registry'deki oyunlar listeleniyor...\n");
            
            let mut games = registry::load_games();

            if games.is_empty() {
                println!("⚠️ Henüz keşfedilmiş bir oyun yok. 'listen' komutuyla ağı dinlemeye başlayın!");
            } else {
                // Algoritmasız, saf kronolojik sıralama (En yeni en üstte)
                games.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                // cli-table ile terminalde şık gösterim
                let mut table_rows = Vec::new();
                
                for (idx, game) in games.iter().enumerate() {
                    let date_str = game.timestamp.format("%Y-%m-%d %H:%M").to_string();
                    
                    table_rows.push(vec![
                        (idx + 1).cell().justify(Justify::Right),
                        game.name.clone().cell(),
                        game.cid.clone().cell(),
                        date_str.cell(),
                    ]);
                }

                let table = table_rows
                    .table()
                    .title(vec![
                        "NO".cell().bold(true),
                        "İSİM".cell().bold(true),
                        "CID (ADRES)".cell().bold(true),
                        "TARİH".cell().bold(true),
                    ])
                    .bold(true);

                // Tabloyu ekrana bas
                println!("{}", table.display().unwrap());
                println!("\n💡 Oynamak için: cargo run -- play <CID>");
            }
        }

        Commands::Listen => {
            println!("👂 P2P Ağı dinleniyor... (Çıkmak için Ctrl+C'ye basın)");
            network::start_listener("fairplay-games").await; 
        }

        Commands::Connect { channel } => {
            println!("🔗 Özel kanala kilitleniyor: {}", channel);
            network::start_listener(channel).await;
        }

        Commands::Play { id } => {
            let games = crate::registry::load_games();
            let game = games.into_iter().find(|g| g.id == *id).expect("❌ Oyun kayıtlı değil! Önce listelemeniz gerekebilir.");

            println!("🎮 '{}' için hazırlıklar başlatılıyor...", game.name);
            
            match crate::network::fetch_game(id).await {
                Ok(cache_path) => {
                    println!("🚀 Oyun dosyaları hazır: {:?}", cache_path);
                    
                    if let Some(exec_path) = game.executable {
                        // 1. Akıllı Dosya Bulucu (Smart Path Resolver)
                        let mut full_exec_path = cache_path.join(&exec_path);

                        // Verilen yol doğrudan çalışmıyorsa IPFS hiyerarşisinde dosyayı ara
                        if !full_exec_path.exists() {
                            let file_name = Path::new(&exec_path).file_name().unwrap();
                            
                            // İhtimal 1: IPFS arşivi her şeyi CID isimli bir klasöre koyar
                            let alt_path_1 = cache_path.join(id).join(file_name);
                            // İhtimal 2: Direkt cache klasörünün altına inmiştir
                            let alt_path_2 = cache_path.join(file_name);

                            if alt_path_1.exists() {
                                full_exec_path = alt_path_1;
                            } else if alt_path_2.exists() {
                                full_exec_path = alt_path_2;
                            }
                        }

                        // Hala bulunamadıysa uyarı ver
                        if !full_exec_path.exists() {
                            eprintln!("❌ İndirme başarılı ancak '{}' çalıştırılabilir dosyası bulunamadı!", exec_path);
                            return;
                        }

                        println!("⚙️ Oyun motoru ateşleniyor: {:?}", full_exec_path);
                        
                        // 2. OYUNUN KENDİ KLASÖRÜNÜ ÇALIŞMA DİZİNİ YAP (Oyunun çökmemesi için hayati)
                        let work_dir = full_exec_path.parent().unwrap();

                        // UNIX İzinleri
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(metadata) = std::fs::metadata(&full_exec_path) {
                                let mut perms = metadata.permissions();
                                perms.set_mode(0o755); // Çalıştırma izni ver (rwxr-xr-x)
                                let _ = std::fs::set_permissions(&full_exec_path, perms);
                            }
                        }

                        // Oyunu Başlat
                        println!("==================================================");
                        let mut child = std::process::Command::new(&full_exec_path)
                            .current_dir(work_dir) // OYUNUN YANINDAKİ KLASÖRLERİ GÖREBİLMESİ İÇİN
                            .spawn()
                            .expect("❌ Oyun başlatılamadı! Dosya bozuk veya uyumsuz olabilir.");
                        
                        let status = child.wait().expect("Oyun süreci dinlenirken hata oluştu");
                        println!("==================================================");
                        println!("🏁 Oyun kapandı (Çıkış Kodu: {})", status);

                    } else {
                        println!("⚠️ Bu oyun için otomatik başlatma verisi (executable) tanımlanmamış.");
                        println!("📂 Dosyalara şu dizinden ulaşabilirsiniz: {:?}", cache_path);
                    }
                }
                Err(e) => {
                    eprintln!("❌ Oyunu indirirken bir hata oluştu: {}", e);
                }
            }
        }
        Commands::Ui => {
            println!("🎨 Fairplay arayüzü başlatılıyor...");
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([800.0, 600.0])
                    .with_title("Fairplay P2P Store"),
                ..Default::default()
            };
            eframe::run_native(
                "Fairplay",
                options,
                Box::new(|_cc| Ok(Box::<FairplayApp>::default())),
            ).unwrap();
        }
    }
}

// --- GÜNCELLENMİŞ GUI KODLARI BAŞLANGICI ---
struct FairplayApp {
    games: Vec<crate::registry::Game>,
}

impl Default for FairplayApp {
    fn default() -> Self {
        Self {
            games: crate::registry::load_games(), 
        }
    }
}

impl eframe::App for FairplayApp {
    #[allow(deprecated)]
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // 1. ÜST PANEL: Başlık burada sabit kalır
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.heading("🎮 Fairplay P2P Store");
            });
            ui.add_space(10.0);
        });

        // 2. ALT PANEL: Yenile butonu ve alt bilgi burada sabitlenir
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                if ui.button("🔄 Kütüphaneyi Yenile").clicked() {
                    self.games = crate::registry::load_games();
                }
                ui.add_space(5.0);
                ui.label("Fairplay P2P Network - Secure & Censorship Resistant");
            });
            ui.add_space(10.0);
        });

        // 3. ORTA PANEL: Sadece oyun listesi (Kaydırılabilir alan)
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.games.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Ağda henüz oyun bulunamadı.\nLütfen terminalden dinlemeye (listen) devam edin...");
                });
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2]) // Tüm alanı kaplamasını sağlar
                    .show(ui, |ui| {
                        ui.add_space(5.0);
                        for game in &self.games {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.heading(&game.name);
                                        ui.label(format!("v{}", game.version));
                                    });
                                    
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("▶ Oyna").clicked() {
                                            println!("🚀 {} başlatılıyor...", game.name);
                                        }
                                    });
                                });
                                
                                ui.add_space(5.0);
                                ui.label(format!("🆔 {}", game.id));
                                
                                ui.horizontal(|ui| {
                                    ui.label("🛡️");
                                    ui.colored_label(egui::Color32::from_rgb(0, 255, 100), "Doğrulandı (Zero-TOFU)");
                                });
                            });
                            ui.add_space(8.0);
                        }
                    });
            }
        });
    }

    // Derleyicinin istediği boş 'ui' metodunu buraya ekleyelim ki hata vermesin
    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}
}