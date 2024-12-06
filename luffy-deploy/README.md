## Build deb package

cd luffy-deploy

```bash
# build for all components
sudo sh ./scripts/build-deb.sh 
# build for luffy-gateway
sudo sh ./scripts/build-deb.sh luffy-gateway
```

## Install deb package

```bash
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
