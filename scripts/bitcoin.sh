val_of_platform() {
  case $1 in
        'darwin'*)
          echo 'x86_64-apple-darwin'
          ;;
        'x86_64'*)
          echo 'x86_64-linux-gnu'
          ;;
        'arm'*)
          echo 'arm-linux-gnueabihf'
          ;;
        'aarch64'*)
          echo 'aarch64-linux-gnu'
          ;;
        *)
          echo 'unknown'
          ;;
    esac
}
platform=$(val_of_platform $(uname -m))
echo "Platform: ${platform} to $(uname -m) architecture"

if [[ $OSTYPE == 'darwin'* ]]
then
    # URL to Bitcoin v22.0
    curl -o bitcoin.rb https://raw.githubusercontent.com/Homebrew/homebrew-core/fa6b4765d81016166f6de2bdad96cfe914c1439f/Formula/bitcoin.rb
    HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK=1 brew install ./bitcoin.rb
else
    curl -o bitcoin-22.0-${platform}.tar.gz https://bitcoincore.org/bin/bitcoin-core-22.0/bitcoin-22.0-${platform}.tar.gz
    tar xzf bitcoin-22.0-${platform}.tar.gz
    sudo install -m 0755 -o root -g root -t /usr/local/bin bitcoin-22.0/bin/*
fi
