mod registry;
mod network;

use eframe::egui;
use clap::{Parser, Subcommand};
use cli_table::{format::Justify, Cell, Style, Table};
use serde::Deserialize;
use std::fs;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender, UnboundedReceiver};
use std::path::PathBuf;

// --- STATE VE MESAJLAŞMA ---
pub enum AppMessage {
    DownloadProgress { game_id: String, progress: f32 },
    DownloadComplete { game_id: String, path: PathBuf },
    GameStarted { game_id: String },
    GameExited { game_id: String, exit_code: i32 },
    Error(String),
}

#[derive(PartialEq)]
pub enum AppState {
    Idle,
    Downloading { game_id: String, progress: f32 },
    Running { game_id: String },
}

// --- GÜNCELLENMİŞ GUI KODLARI ---
struct FairplayApp {
    games: Vec<crate::registry::Game>,
    state: AppState,
    msg_receiver: UnboundedReceiver<AppMessage>,
    msg_sender: UnboundedSender<AppMessage>,
}

impl Default for FairplayApp {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            games: crate::registry::load_games(), 
            state: AppState::Idle,
            msg_receiver: rx,
            msg_sender: tx,
        }
    }
}

impl eframe::App for FairplayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());
        
        while let Ok(msg) = self.msg_receiver.try_recv() {
            match msg {
                AppMessage::DownloadProgress { game_id, progress } => {
                    self.state = AppState::Downloading { game_id, progress };
                    ctx.request_repaint(); 
                }
                AppMessage::DownloadComplete { game_id: _, path: _ } => {
                    self.state = AppState::Idle;
                    ctx.request_repaint();
                }
                AppMessage::GameStarted { game_id } => {
                    self.state = AppState::Running { game_id };
                    ctx.request_repaint();
                }
                AppMessage::GameExited { .. } | AppMessage::Error(_) => {
                    self.state = AppState::Idle;
                    ctx.request_repaint();
                }
            }
        }

        egui::Panel::top("top_panel").frame(
            egui::Frame::default().fill(egui::Color32::from_rgb(20, 20, 25)).inner_margin(15.0)
        ).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("🎮 FAIRPLAY").size(24.0).color(egui::Color32::from_rgb(0, 255, 100)).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔄 Yenile").clicked() {
                        self.games = crate::registry::load_games();
                    }
                });
            });
        });

        egui::Panel::bottom("bottom_panel").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Fairplay P2P Network - Secure & Censorship Resistant").small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    match &self.state {
                        AppState::Downloading { .. } => { ui.label("📥 Arka planda indirme yapılıyor..."); },
                        AppState::Running { .. } => { ui.label("🟢 Oyun çalışıyor..."); },
                        AppState::Idle => { ui.label("✅ Sistem hazır"); },
                    }
                });
            });
            ui.add_space(5.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.games.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Ağda henüz oyun bulunamadı.\nLütfen terminalden 'listen' komutuyla dinlemeye devam edin...").color(egui::Color32::DARK_GRAY));
                });
            } else {
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    ui.add_space(10.0);
                    for game in &self.games {
                        egui::Frame::group(ui.style())
                            .fill(egui::Color32::from_rgb(30, 30, 35))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.heading(egui::RichText::new(&game.name).strong().size(18.0));
                                    ui.label(egui::RichText::new(format!("Sürüm: v{} | Geliştirici: {}...", game.version, &game.developer_pubkey[..20])).small().color(egui::Color32::GRAY));
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label("🛡️");
                                        ui.label(egui::RichText::new("Doğrulandı (Zero-TOFU)").color(egui::Color32::from_rgb(0, 255, 100)).small());
                                    });
                                });
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    match &self.state {
                                        AppState::Downloading { game_id, progress } if game_id == &game.id => {
                                            ui.add(egui::ProgressBar::new(*progress).show_percentage().animate(true).desired_width(150.0));
                                        },
                                        AppState::Running { game_id } if game_id == &game.id => {
                                            ui.add(egui::Spinner::new());
                                            ui.label(egui::RichText::new("Çalışıyor").color(egui::Color32::LIGHT_GREEN));
                                        },
                                        _ => {
                                            let is_downloaded = crate::registry::is_game_in_cache(&game.id);
                                            let btn_text = if is_downloaded { "▶ OYNA" } else { "⬇ İNDİR" };
                                            
                                            let mut btn = egui::Button::new(egui::RichText::new(btn_text).strong().size(16.0)).min_size(egui::vec2(100.0, 35.0));
                                            if self.state != AppState::Idle { btn = btn.sense(egui::Sense::hover()); }

                                            if ui.add(btn).clicked() && self.state == AppState::Idle {
                                                let tx = self.msg_sender.clone();
                                                let game_clone = game.clone();
                                                
                                                tokio::spawn(async move {
                                                    if !is_downloaded {
                                                        if let Ok(path) = crate::network::fetch_game(&game_clone.id, Some(tx.clone())).await {
                                                            let _ = tx.send(AppMessage::DownloadComplete { game_id: game_clone.id.clone(), path });
                                                        } else {
                                                            let _ = tx.send(AppMessage::Error("İndirme başarısız".into()));
                                                        }
                                                    } else {
                                                        let _ = tx.send(AppMessage::GameStarted { game_id: game_clone.id.clone() });
                                                        if let Err(e) = crate::network::launch_game(&game_clone) {
                                                            let _ = tx.send(AppMessage::Error(e.to_string()));
                                                        }
                                                        let _ = tx.send(AppMessage::GameExited { game_id: game_clone.id.clone(), exit_code: 0 });
                                                    }
                                                });
                                            }
                                        }
                                    }
                                });
                            });
                        });
                        ui.add_space(8.0);
                    }
                });
            }
        });
    }

    fn ui(&mut self, _ui: &mut egui::Ui, _frame: &mut eframe::Frame) {}
}

// --- CLI YAPILANDIRMA ---
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
            
            let signature = private_key.sign("fairplay-namespace", ssh_key::HashAlg::Sha256, payload.as_bytes()).unwrap();
            
            let cert_str = signature.to_string(); 

            println!("✅ Game developer certificate created!");
            println!("Game ID: {}", id);
            println!("Certificate (owned by game developer):\n{}", cert_str);
        }
        Commands::Publish { config_file } => {
            let file_content = fs::read_to_string(config_file)
                .expect("❌ JSON konfigürasyon dosyası okunamadı! Dosya yolunu kontrol edin.");
            
            let config: PublishConfig = serde_json::from_str(&file_content)
                .expect("❌ JSON dosyası ayrıştırılamadı. Virgülleri veya formatı kontrol edin.");

            println!("🚀 Process started: {} (v{})", config.name, config.version);
            
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
                games.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                let mut table_rows = Vec::new();
                
                for (idx, game) in games.iter().enumerate() {
                    let date_str = game.timestamp.format("%Y-%m-%d %H:%M").to_string();
                    
                    table_rows.push(vec![
                        (idx + 1).cell().justify(Justify::Right),
                        game.name.clone().cell(),
                        game.id.clone().cell(), 
                        date_str.cell(),
                    ]);
                }

                let table = table_rows
                    .table()
                    .title(vec![
                        "NO".cell().bold(true),
                        "İSİM".cell().bold(true),
                        "OYUN ID (UUID)".cell().bold(true), 
                        "TARİH".cell().bold(true),
                    ])
                    .bold(true);

                println!("{}", table.display().unwrap());
                println!("\n💡 Oynamak için: cargo run -- play <OYUN_ID>"); 
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
            let game = games.into_iter().find(|g| g.id == *id).expect("❌ Oyun kayıtlı değil!");
            
            if let Ok(_path) = crate::network::fetch_game(id, None).await {
                if let Err(e) = crate::network::launch_game(&game) {
                    eprintln!("❌ Başlatma hatası: {}", e);
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