## Log in to ECR

```yaml
aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin http://600627348446.dkr.ecr.us-east-1.amazonaws.com/
```

## Build image

```yaml
docker build --platform linux/amd64 -t iii/bun .
```

## Tag Image

```yaml
docker tag iii/bun:latest 600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/bun:latest
```

## Push image

```yaml
docker push 600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/bun:latest
```

```
docker build --platform linux/amd64 -t iii/bun .
docker tag iii/bun:latest 600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/bun:latest
docker push 600627348446.dkr.ecr.us-east-1.amazonaws.com/iii/bun:latest
```
