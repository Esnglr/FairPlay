# Fairplay - Otomasyon ve Kullanım Kılavuzu

Bu proje, IPFS ve P2P ağ işlemlerini kolaylaştırmak için bir `Makefile` içerir. Aşağıdaki komutları kullanarak kurulum, test ve derleme işlemlerini hızlıca yapabilirsiniz.

## Başlangıç ve Kurulum

Projeyi ve ağ altyapısını ilk kez kurarken aşağıdaki adımları izleyin:

* **IPFS Kurulumu:**
  IPFS'i başlatmak ve yayın/abonelik (PubSub) özelliğini kalıcı olarak aktif etmek için:
  ```bash
  make ipfs-setup
   ```
* **Güvenlik Duvarı Ayarları:**
  IPFS swarm bağlantıları ve Peer keşfi için gerekli portları (4001) açar.
  (Uyarı: Bu komut sudo yetkisi gerektirir. Secureblue/toolbx kullanıyorsanız, bu komutu toolbx dışında, ana sisteminizde çalıştırmanız gerekebilir.)
    ```bash
  make setup-firewall
   ```
* **IPFS Daemon'u Başlatma:**
  Arka planda takılı kalan eski IPFS işlemlerini temizler ve IPFS daemon'unu temiz bir şekilde yeniden başlatır:
   ```bash
  make run-daemon
   ```
## Ağ Testleri

IPFS PubSub üzerinden iletişimin çalışıp çalışmadığını test etmek için iki farklı terminal kullanabilirsiniz:

1. **Dinleyiciyi Başlatın:** (Ağdaki mesajları görmek için)
   ```bash
   make test-sub
   ```

2. **Test Mesajı Gönderin:** (Diğer terminalden)
   ```bash
   make test-pub
   ```

(Eğer bağlantı başarılıysa, ilk terminalde "Test başarılı" mesajını görmelisiniz.)

## Proje Yönetimi

* **Oyun Duyurusu Yapma (Test):**
  Fairplay üzerinden test.json dosyasını kullanarak ağa örnek bir oyun duyurusu yapmak için:
     ```bash
   make publish-test
   ```
* **Projeyi Derleme ve Sisteme Kurma:**
  Rust projesini en optimize (release) modunda derler ve çalıştırılabilir fairplay dosyasını kullanıcı dizininizdeki ~/.local/bin/ klasörüne kopyalar:
  ```bash
   make install
   ```
  
