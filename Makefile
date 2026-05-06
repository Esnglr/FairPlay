.PHONY: install ipfs-setup run-daemon setup-firewall test-pub test-sub publish-game

# IPFS ilk kurulumu ve PubSub'ın kalıcı olarak aktifleştirilmesi
ipfs-setup:
	@echo "IPFS başlatılıyor ve Pubsub kalıcı olarak aktifleştiriliyor..."
	ipfs init || true
	ipfs config Pubsub.Enabled --json true

# IPFS daemon'unu temiz bir şekilde başlatma
run-daemon:
	@echo "Eski IPFS daemon işlemleri sonlandırılıyor..."
	-pkill ipfs || true
	@echo "IPFS daemon başlatılıyor..."
	ipfs daemon

# Firewall ayarları (Peer keşfi ve IPFS swarm bağlantıları için)
setup-firewall:
	@echo "IPFS için portlar (4001) ve mdns servisi güvenlik duvarında açılıyor..."
	@echo "NOT: Sudo yetkisi gerektiği için bu komut toolbx içinde patlayabilir!"
	firewall-cmd --add-port=4001/tcp
	firewall-cmd --add-port=4001/udp
	firewall-cmd --add-service=mdns

# Ağda yayın (publish) yapıldığını test etmek için
test-pub:
	@echo "fairplay-games frekansına test mesajı gönderiliyor..."
	echo "Test başarılıç" | ipfs pubsub pub fairplay-games

# Ağdaki yayınları dinlemek (subscribe) için (Senin "tuna sub" dediğin işlem)
test-sub:
	@echo "fairplay-games frekansı dinleniyor (Çıkmak için Ctrl+C)..."
	ipfs pubsub sub fairplay-games

# Fairplay üzerinden JSON konfigürasyonu ile oyunu ağa duyurma
publish-test:
	@echo "Oyun JSON konfigürasyonu kullanılarak ağa duyuruluyor..."
	cargo run -- publish test.json

# Projeyi derleyip PATH'e kopyalama (Secureblue için ~/.local/bin/ dizini kullanıldı)
install:
	@echo "Fairplay release modunda derleniyor..."
	cargo build --release
	@echo "Çalıştırılabilir dosya sisteme kopyalanıyor (~/.local/bin/)..."
	mkdir -p ~/.local/bin
	cp target/release/fairplay ~/.local/bin/