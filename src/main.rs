mod registry;
mod network;
use clap::{Parser, Subcommand};

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
        Commands::Publish { name, path, channel } => {
            println!("🚀 İşlem başlatıldı: {} (Yol: {}, Kanal: {})", name, path, channel);
            // channel parametresini fonksiyona iletiyoruz
            if let Err(e) = network::publish_game(name, path, channel).await {
                eprintln!("Yayınlama başarısız oldu: {}", e);
            }
        }
        Commands::List => {
            println!("🔍 Yerel registry'deki oyunlar listeleniyor...\n");
            
            // Veritabanından oyunları çekiyoruz
            let mut games = registry::load_games();

            if games.is_empty() {
                println!("⚠️ Henüz keşfedilmiş bir oyun yok. 'listen' komutuyla ağı dinlemeye başlayın!");
            } else {
                // Algoritmasız, saf kronolojik sıralama (En yeni en üstte)
                games.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

                // Tablo başlıkları
                println!("{:<5} {:<25} {:<50} {:<20}", "NO", "İSİM", "CID (ADRES)", "TARİH");
                println!("{}", "-".repeat(105));

                for (idx, game) in games.iter().enumerate() {
                    let date_str = game.timestamp.format("%Y-%m-%d %H:%M").to_string();
                    
                    println!("{:<5} {:<25} {:<50} {:<20}", 
                        idx + 1, 
                        game.name, 
                        game.cid, 
                        date_str
                    );
                }
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
            network::start_listener(channel).await;
        } Commands::Play { id } => {
            println!("🎮 Oyun başlatılıyor (ID: {})...", id);
        }
    }
}
