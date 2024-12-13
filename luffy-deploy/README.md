## Build deb package

Make sure you docker is running.

```bash
cd luffy-deploy
# build for all components
sudo sh ./scripts/build-deb.sh 
# build for luffy-gateway
sudo sh ./scripts/build-deb.sh luffy-gateway
```

## Install deb package

```bash
wget https://github.com/MarineThinking/luffy/releases/download/v0.4.1/luffy-gateway_0.4.1-1_amd64.deb
wget https://github.com/Marinethinking/luffy/releases/download/v0.4.1/luffy-launcher_0.4.1-1_arm64.deb
sudo dpkg -i dist/*.deb
```

## Run service

```bash
sudo systemctl start luffy-gateway
sudo journalctl -u luffy-gateway -f
```


## Remove deb package

```bash
sudo dpkg -r luffy-gateway
# remove all files, including configuration, without --purge, nest installation will not copy configuration
sudo dpkg --purge luffy-gateway 
```
