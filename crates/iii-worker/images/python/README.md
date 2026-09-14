# Python Sandbox Image

This directory contains the Dockerfile for the Python sandbox image used as rootfs for iii managed workers.

## Targets

| Target | Contents | Suggested tag |
| --- | --- | --- |
| `runtime` (default) | CPython 3.12 with pip and venv, CA certificates and the non-root `python-user` user. No build toolchain or linters. | `iiidev/python:latest` |
| `builder` | Runtime plus `build-essential` and `libssl-dev` for native sdist builds and the existing development tools. | `iiidev/python:builder` |

Use the builder when a dependency requires native compilation instead of a prebuilt binary or wheel. Runtime and builder share the same base image.

## Building the Images

Run these from the project root:

```bash
docker build --pull -t iiidev/python:latest crates/iii-worker/images/python
docker build --target builder -t iiidev/python:builder crates/iii-worker/images/python
```

These commands build local images. Publish both tags to an accessible registry before using them with iii: workers pull their rootfs from the registry, not from the local Docker daemon.

## Publishing

The [image publishing workflow](../../../../.github/workflows/docker-worker-images.yml) validates both targets on pull requests. Changes to these images or the workflow on `main` publish `latest` (runtime) and `builder` to Docker Hub for `linux/amd64` and `linux/arm64`. The workflow can also be run manually on `main`. Builds pull the current upstream base image.

## Using the Builder Image

Set `runtime.base_image` in the worker's existing `iii.worker.yaml`, keeping its install and start scripts:

```yaml
runtime:
  base_image: docker.io/iiidev/python:builder
```

## Running the Container

```bash
docker run -it --name python iiidev/python:latest
```

### Options

- `--name python`: Names the container for easier reference

## Accessing the Container

To access a shell inside the running container:

```bash
docker exec -it python bash
```

## Stopping and Cleaning Up

```bash
docker stop python                # Stop the container
docker rm python                  # Remove the container
docker rmi iiidev/python:latest   # Remove the image (optional)
```

## Customization

### Adding Additional Python Packages

Add them to the stage that needs them:

```dockerfile
RUN pip install --no-cache-dir \
    numpy \
    pandas \
    matplotlib
```

### Mounting Local Files

To access your local files inside the container:

```bash
docker run -it -v $(pwd)/your_code:/home/python-user/work --name python iiidev/python:latest
```

## Troubleshooting

1. Check the logs: `docker logs python`
2. Verify the container is running: `docker ps | grep python`
