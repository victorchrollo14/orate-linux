PREFIX ?= $(HOME)/.local
BIN_DIR := $(PREFIX)/bin
DATA_DIR := $(PREFIX)/share
SCHEMA_DIR := $(DATA_DIR)/glib-2.0/schemas
DESKTOP_DIR := $(DATA_DIR)/applications
DBUS_SERVICE_DIR := $(DATA_DIR)/dbus-1/services
ICON_DIR := $(DATA_DIR)/icons/hicolor
EXT_UUID := orate@orate.app
EXT_DIR := $(DATA_DIR)/gnome-shell/extensions/$(EXT_UUID)

.PHONY: build install install-app install-extension uninstall dev logs deps

# Build deps on Fedora:
#   sudo dnf install gtk4-devel libsecret-devel gstreamer1-devel \
#       gstreamer1-plugins-base-devel gstreamer1-plugins-good \
#       pipewire-gstreamer wl-clipboard pkgconf-pkg-config
deps:
	@echo "Run: sudo dnf install gtk4-devel libsecret-devel gstreamer1-devel gstreamer1-plugins-base-devel gstreamer1-plugins-good pipewire-gstreamer wl-clipboard pkgconf-pkg-config"

build:
	cd app && cargo build --release

install: install-app install-extension

install-app: build
	install -Dm755 app/target/release/orate $(BIN_DIR)/orate
	install -Dm644 app/data/org.orate.app.gschema.xml $(SCHEMA_DIR)/org.orate.app.gschema.xml
	install -Dm644 app/data/com.orate.App.desktop $(DESKTOP_DIR)/com.orate.App.desktop
	install -Dm644 app/data/icon.svg $(ICON_DIR)/scalable/apps/com.orate.App.svg
	install -Dm644 app/data/icon.png $(ICON_DIR)/256x256/apps/com.orate.App.png
	-gtk-update-icon-cache -f -t $(ICON_DIR)
	mkdir -p $(DBUS_SERVICE_DIR)
	sed 's|@BIN_PATH@|$(BIN_DIR)/orate|g' app/data/com.orate.App.Service.service.in \
		> $(DBUS_SERVICE_DIR)/com.orate.App.Service.service
	chmod 644 $(DBUS_SERVICE_DIR)/com.orate.App.Service.service
	glib-compile-schemas $(SCHEMA_DIR)
	-busctl --user call org.freedesktop.DBus / org.freedesktop.DBus ReloadConfig

install-extension:
	mkdir -p $(EXT_DIR) $(EXT_DIR)/schemas
	install -Dm644 extension/metadata.json $(EXT_DIR)/metadata.json
	install -Dm644 extension/extension.js $(EXT_DIR)/extension.js
	install -Dm644 extension/stylesheet.css $(EXT_DIR)/stylesheet.css
	install -Dm644 extension/schemas/org.gnome.shell.extensions.orate.gschema.xml \
		$(EXT_DIR)/schemas/org.gnome.shell.extensions.orate.gschema.xml
	glib-compile-schemas $(EXT_DIR)/schemas
	-gnome-extensions enable $(EXT_UUID)
	@echo
	@echo "Extension installed. Log out and back in (Wayland) to load it."
	@echo "Then run 'make logs' to tail extension output."

uninstall:
	rm -f $(BIN_DIR)/orate
	rm -f $(SCHEMA_DIR)/org.orate.app.gschema.xml
	rm -f $(DESKTOP_DIR)/com.orate.App.desktop
	rm -f $(DBUS_SERVICE_DIR)/com.orate.App.Service.service
	rm -f $(ICON_DIR)/scalable/apps/com.orate.App.svg
	rm -f $(ICON_DIR)/256x256/apps/com.orate.App.png
	-gtk-update-icon-cache -f -t $(ICON_DIR)
	-glib-compile-schemas $(SCHEMA_DIR)
	rm -rf $(EXT_DIR)

# Compiles the GSettings schema into a scratch dir and points GSettings at it,
# so preferences persist when running straight from the source tree.
DEV_SCHEMA_DIR := $(CURDIR)/app/target/schemas

dev:
	mkdir -p $(DEV_SCHEMA_DIR)
	cp app/data/org.orate.app.gschema.xml $(DEV_SCHEMA_DIR)/
	glib-compile-schemas $(DEV_SCHEMA_DIR)
	cd app && GSETTINGS_SCHEMA_DIR=$(DEV_SCHEMA_DIR) cargo run

# App logs (recorder, D-Bus service, transcription):
#   $XDG_STATE_HOME/orate/orate.log   (defaults to ~/.local/state/orate/orate.log)
# Extension logs (waveform pill, keybinding, paste):
#   journalctl --user -f /usr/bin/gnome-shell
# A debug copy of the most recent recording is kept at:
#   ~/.local/state/orate/last_recording.flac
logs:
	@echo "==> tailing app log + gnome-shell extension log (Ctrl+C to stop)"
	@mkdir -p $${XDG_STATE_HOME:-$$HOME/.local/state}/orate
	@touch $${XDG_STATE_HOME:-$$HOME/.local/state}/orate/orate.log
	tail -F $${XDG_STATE_HOME:-$$HOME/.local/state}/orate/orate.log & \
		journalctl --user -f /usr/bin/gnome-shell | grep --line-buffered -i orate; \
		kill %1 2>/dev/null

applog:
	tail -F $${XDG_STATE_HOME:-$$HOME/.local/state}/orate/orate.log

extlog:
	journalctl --user -f /usr/bin/gnome-shell | grep --line-buffered -i orate
