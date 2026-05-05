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
    },
    /// Ağdan duyulan mevcut oyunları listele
    List,
    /// Ağı sürekli dinleyerek yeni oyunları keşfeder (Kapatana kadar çalışır)
    Listen, // <--- EKSİK OLAN SATIR BURASIYDI!
    /// Bir oyunu ID ile indir ve izole ortamda (sandbox) çalıştır
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
        Commands::Publish { name, path } => {
            println!("🚀 İşlem başlatıldı: {} (Yol: {})", name, path);
            if let Err(e) = network::publish_game(name, path).await {
                eprintln!("Yayınlama başarısız oldu: {}", e);
            }
        }
        Commands::List => {
            println!("🔍 Yerel registry'deki oyunlar listeleniyor...");
            // Issue #6'da buraya veritabanı okuma kodunu yazacağız
        }
        // YENİ EKLENEN KISIM:
        Commands::Listen => {
            println!("👂 P2P Ağı dinleniyor... (Çıkmak için Ctrl+C'ye basın)");
            // await kullandığımız için program burada asılı kalır ve kapanmaz
            network::start_listener().await; 
        }
        Commands::Play { id } => {
            println!("🎮 Oyun başlatılıyor (ID: {})...", id);
        }
    }
}
