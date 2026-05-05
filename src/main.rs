mod registry;
mod network;
use clap::{Parser, Subcommand};
use cli_table::{format::Justify, Cell, Style, Table};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Yeni bir oyunu IPFS ağına yükle ve duyur
    Publish {
        /// Oyunun adı
        #[arg(short, long)]
        name: String,
        /// Oyun dosyası veya dizininin yolu
        #[arg(short, long)]
        path: String,
        #[arg(short, long, default_value = "fairplay-games")]
        channel: String,
        #[arg(short, long)]
        executable: Option<String>,
    },
    /// Ağdan duyulan mevcut oyunları listele
    List,
    /// Ağı sürekli dinleyerek yeni oyunları keşfeder (Kapatana kadar çalışır)
    Listen, // <--- EKSİK OLAN SATIR BURASIYDI!
    /// Bir oyunu ID ile indir ve izole ortamda (sandbox) çalıştır
    Connect {
        channel: String,
    },
    Play {
        /// Oynanacak oyunun kayıt ID'si
        id: String,
    },
}

#[tokio::main]
async fn main() {
    registry::init();

    // DİKKAT: Buradaki tokio::spawn bloğunu tamamen sildik!

    let cli = Cli::parse();

    match &cli.command {
        Commands::Publish { name, path, channel, executable } => {
            println!("🚀 İşlem başlatıldı: {} (Yol: {}, Kanal: {})", name, path, channel);
            // channel parametresini fonksiyona iletiyoruz
            if let Err(e) = network::publish_game(name, path, channel, executable.clone()).await {
                eprintln!("Yayınlama başarısız oldu: {}", e);
            }
        }
        Commands::List => {
            println!("🔍 Yerel registry'deki oyunlar listeleniyor...\n");
            
            // Task 6.1: Veritabanından oyunları RAM'e çekiyoruz[cite: 1]
            let mut games = registry::load_games();

            if games.is_empty() {
                println!("⚠️ Henüz keşfedilmiş bir oyun yok. 'listen' komutuyla ağı dinlemeye başlayın!");
            } else {
                // Task 6.2: Algoritmasız, saf kronolojik sıralama (En yeni en üstte)[cite: 1]
                games.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                // Task 6.3: cli-table ile terminalde şık gösterim
                let mut table_rows = Vec::new();
                
                for (idx, game) in games.iter().enumerate() {
                    let date_str = game.timestamp.format("%Y-%m-%d %H:%M").to_string();
                    
                    // Her bir satırı tabloya ekliyoruz
                    table_rows.push(vec![
                        (idx + 1).cell().justify(Justify::Right),
                        game.name.clone().cell(),
                        game.cid.clone().cell(),
                        date_str.cell(),
                    ]);
                }

                // Tablonun başlıkları ve genel stili
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
        // YENİ EKLENEN KISIM:
        Commands::Listen => {
            println!("👂 P2P Ağı dinleniyor... (Çıkmak için Ctrl+C'ye basın)");
            // await kullandığımız için program burada asılı kalır ve kapanmaz
            network::start_listener("fairplay-games").await; 
        }
        Commands::Connect { channel } => {
            println!("Ozel kanala kilitleniyor: {}", channel);
            network::start_listener(channel).await;}
        Commands::Play { id } => {
            let mut games = crate::registry::load_games();
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
                            let file_name = std::path::Path::new(&exec_path).file_name().unwrap();
                            
                            // İhtimal 1: IPFS arşivi her şeyi CID isimli bir klasöre koyar (cache/CID/CID/dwarfort)
                            let alt_path_1 = cache_path.join(id).join(file_name);
                            // İhtimal 2: Direkt cache klasörünün altına inmiştir (cache/CID/dwarfort)
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
                            .current_dir(work_dir) // <--- OYUNUN YANINDAKİ KLASÖRLERİ GÖREBİLMESİ İÇİN
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
    }
}
