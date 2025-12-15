## Log in to ECR

```yaml
aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin http://600627348446.dkr.ecr.us-east-1.amazonaws.com/
```

## Build image

```yaml
docker build -f DockerfileBun -t iii/bun .
```

## Tag Image

```yaml
docker tag iii/bun:latest http://600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/development:latest
```

## Push image

```yaml
docker push http://600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/development:latest
```
