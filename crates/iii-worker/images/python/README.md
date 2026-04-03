# Python Sandbox Image

This directory contains the Dockerfile for the Python sandbox image used as rootfs for iii managed workers.

## Features

- Latest Python version
- Common Python development packages pre-installed
- Non-root user for improved security

## Building the Image

To build the image, run the following command from the project root:

```bash
docker build -t iiidev/python -f Dockerfile .
```

## Running the Container

```bash
docker run -it --name python iiidev/python
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
# Stop the container
docker stop python

# Remove the container
docker rm python

# Remove the image (optional)
docker rmi iiidev/python
```

## Customization

### Adding Additional Python Packages

You can customize the Dockerfile to include additional Python packages:

```dockerfile
RUN pip install --no-cache-dir \
    numpy \
    pandas \
    matplotlib
```

### Mounting Local Files

To access your local files inside the container:

```bash
docker run -it -v $(pwd)/your_code:/home/python-user/work --name python iiidev/python
```

## Troubleshooting

1. Check the logs: `docker logs python`
2. Verify the container is running: `docker ps | grep python`
