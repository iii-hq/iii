# Node.js Sandbox Image

This directory contains the Dockerfile for the Node.js sandbox image used as rootfs for iii managed workers.

## Features

- Node.js 20.x LTS (Latest LTS version)
- NPM package manager
- TypeScript and ts-node
- Development tools (nodemon, eslint, prettier)
- Built-in non-root 'node' user for improved security

## Building the Image

To build the image, run the following command from the project root:

```bash
docker build -t iiidev/node -f Dockerfile .
```

## Running the Container

```bash
docker run -it --name node iiidev/node
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
# Stop the container
docker stop node

# Remove the container
docker rm node

# Remove the image (optional)
docker rmi iiidev/node
```

## Customization

### Adding Additional NPM Packages

You can customize the Dockerfile to include additional NPM packages:

```dockerfile
RUN npm install -g \
    jest \
    webpack \
    webpack-cli
```

### Mounting Local Files

To access your local files inside the container:

```bash
docker run -it -v $(pwd)/your_code:/home/node/work --name node iiidev/node
```

## Troubleshooting

1. Check the logs: `docker logs node`
2. Verify the container is running: `docker ps | grep node`
