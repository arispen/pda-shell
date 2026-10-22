# Debian 13 development image for hosts that are not Debian (e.g. macOS).
# It runs the same setup script a Debian developer would run.
FROM debian:trixie

COPY scripts/setup-debian.sh /tmp/setup-debian.sh
RUN /tmp/setup-debian.sh \
    && apt-get install -y --no-install-recommends x11vnc locales \
    && sed -i 's/^# *en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen && locale-gen \
    && rm -rf /var/lib/apt/lists/*

ENV LANG=en_US.UTF-8 \
    CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target

RUN useradd -m -s /bin/bash dev && mkdir -p /cargo /target && chown dev /cargo /target
USER dev
WORKDIR /src
