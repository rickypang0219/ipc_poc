# Podman funciton

```bash
# run AlmaLinux on MacOS
podman run --rm --platform linux/amd64 docker.io/almalinux/9-minimal uname -m

# Build Container
podman build --platform linux/amd64 -t localhost/ipc_poc:latest -f Containerfile .

# Start Podman Container
podman compose up -d

# Shutdown Podman Container
podman compose down

# Shotdown Podman Container (Erase All Data)
podman compose down -v

# Check Podman System DF
podman system df -v

# Clean Dangling Images
podman image prune

# Check Logs from feed/sink
podman compose logs -f feed sink

```
