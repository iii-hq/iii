# Node.js Sandbox Image

This directory contains the Dockerfile for the Node.js sandbox image used as rootfs for iii managed workers.

## Targets

| Target | Contents | Suggested tag |
| --- | --- | --- |
| `runtime` (default) | Node.js 24 with npm and tsx, CA certificates and the non-root `node` user. No build toolchain or linters. | `iiidev/node:latest` |
| `builder` | Runtime plus `build-essential` and `python3` for node-gyp and the existing development tools. | `iiidev/node:builder` |

Use the builder when a dependency requires native compilation instead of a prebuilt binary or wheel. Runtime and builder share the same base image.

## Building the Images

Run these from the project root:

```bash
docker build --pull -t iiidev/node:latest crates/iii-worker/images/node
docker build --target builder -t iiidev/node:builder crates/iii-worker/images/node
```

These commands build local images. Publish both tags to an accessible registry before using them with iii: workers pull their rootfs from the registry, not from the local Docker daemon.

## Publishing

The [image publishing workflow](../../../../.github/workflows/docker-worker-images.yml) validates both targets on pull requests. Changes to these images or the workflow on `main` publish `latest` (runtime) and `builder` to Docker Hub for `linux/amd64` and `linux/arm64`. The workflow can also be run manually on `main`. Builds pull the current upstream base image.

## Using the Builder Image

Set `runtime.base_image` in the worker's existing `iii.worker.yaml`, keeping its install and start scripts:

```yaml
runtime:
  base_image: docker.io/iiidev/node:builder
```

## Running the Container

```bash
docker run -it --name node iiidev/node:latest
```

### Options

- `--name node`: Names the container for easier reference

## Accessing the Container

To access a shell inside the running container:

```bash
docker exec -it node bash
```

## Stopping and Cleaning Up

```bash
docker stop node                # Stop the container
docker rm node                  # Remove the container
docker rmi iiidev/node:latest   # Remove the image (optional)
```

## Customization

### Adding Additional NPM Packages

Add them to the stage that needs them:

```dockerfile
RUN npm install -g \
    jest \
    webpack \
    webpack-cli
```

### Mounting Local Files

To access your local files inside the container:

```bash
docker run -it -v $(pwd)/your_code:/home/node/work --name node iiidev/node:latest
```

## Troubleshooting

1. Check the logs: `docker logs node`
2. Verify the container is running: `docker ps | grep node`
