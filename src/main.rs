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
        /// Oyun dosyası veya dizininin yolu (örn: ./my_game)
        #[arg(short, long)]
        path: String,
    },
    /// Ağdan duyulan mevcut oyunları listele
    List,
    /// Bir oyunu ID ile indir ve izole ortamda (sandbox) çalıştır
    Play {
        /// Oynanacak oyunun kayıt ID'si
        id: String,
    },
}

#[tokio::main]
async fn main() {
    registry::init();

    tokio::spawn(async move {
        network::start_listener().await;
    });

    let cli = Cli::parse();

    // Kullanıcının girdiği komutu eşleştirip ilgili fonksiyona yönlendiriyoruz
    match &cli.command {
        Commands::Publish { name, path } => {
            println!("Yayınlanıyor...\nOyun Adı: {}\nDosya Yolu: {}", name, path);
            // TODO: Issue #5 (IPFS'e ekle ve PubSub ile anons et)
            if let Err(e) = network::publish_game(name, path).await {
                eprintln!("Yayınlama başarısız oldu: {}", e);
            }
        }
        Commands::List => {
            println!("Yerel registry'deki oyunlar listeleniyor...");
            // TODO: Issue #6 (registry.json'u okuyup tablo halinde bas)
        }
        Commands::Play { id } => {
            println!("Oyun başlatılıyor (ID: {})...", id);
            // TODO: Issue #7 ve #9 (IPFS'ten indir ve bwrap ile çalıştır)
        }
    }
}
