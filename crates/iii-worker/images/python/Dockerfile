FROM python:latest

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1 \
    LANG=C.UTF-8 \
    DEBIAN_FRONTEND=noninteractive

ARG USER_NAME="python-user"
ARG USER_UID="1000"
ARG USER_GID="100"

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    git \
    wget \
    libssl-dev \
    ca-certificates \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && useradd -m -s /bin/bash -N -u $USER_UID $USER_NAME \
    && mkdir -p /home/$USER_NAME/work \
    && chown -R $USER_NAME:$USER_GID /home/$USER_NAME \
    && pip install --no-cache-dir --upgrade pip setuptools wheel \
    && pip install --no-cache-dir \
        black \
        flake8 \
        mypy \
        pytest \
        pytest-cov \
        requests \
        ipython

USER $USER_NAME
WORKDIR /home/$USER_NAME/work

CMD ["tail", "-f", "/dev/null"]
