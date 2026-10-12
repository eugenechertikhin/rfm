# rfm build & packaging.
# Targets: debug, release, debian (.deb), arch (pacman package), rpm (.rpm), clean.
# `make` without arguments builds release.

.DEFAULT_GOAL := release

NAME     := rfm
VERSION  := $(shell cat VERSION)
BUILD    := build

# Debian architecture (dpkg on a debian host; uname fallback for cross checks).
DEB_ARCH := $(shell dpkg --print-architecture 2>/dev/null || uname -m)

.PHONY: version debug release debian arch rpm clean

debug: version
	cargo build

release: version
	cargo build --release

# ── Версии ─────────────────────────────────────────────────────────────
# Синхронизирует `version` в Cargo.toml каждого крейта из корневого VERSION.
# Трогает только строку версии пакета (якорь ^version — зависимости задаются
# инлайн-таблицами и под якорь не попадают). Файл переписывается только при
# реальном расхождении — иначе смена mtime зря дёргала бы пересборку cargo.
.PHONY: version
version:
	@v=$$(tr -d '[:space:]' < VERSION); \
	if [ -z "$$v" ]; then echo "error: VERSION file is empty" >&2; exit 1; fi; \
	for c in $(ALL_CRATES); do \
		cur=$$(sed -n 's/^version = "\(.*\)"$$/\1/p' $$c/Cargo.toml); \
		if [ "$$cur" != "$$v" ]; then \
			echo ">> $$c: version $$cur -> $$v"; \
			sed "s/^version = \".*\"$$/version = \"$$v\"/" $$c/Cargo.toml > $$c/Cargo.toml.tmp \
				&& mv $$c/Cargo.toml.tmp $$c/Cargo.toml || exit $$?; \
		fi; \
	done; \
	echo ">> versions in sync ($$v)"

# --- Debian package (requires dpkg-deb; run on a debian-like host) ---
debian: release
	rm -rf $(BUILD)/debian
	install -Dm755 target/release/$(NAME) $(BUILD)/debian/pkg/usr/bin/$(NAME)
	install -Dm644 README.md $(BUILD)/debian/pkg/usr/share/doc/$(NAME)/README.md
	mkdir -p $(BUILD)/debian/pkg/DEBIAN
	sed -e 's/@VERSION@/$(VERSION)/' -e 's/@ARCH@/$(DEB_ARCH)/' \
		packaging/debian/control.in > $(BUILD)/debian/pkg/DEBIAN/control
	dpkg-deb --build --root-owner-group $(BUILD)/debian/pkg \
		$(BUILD)/$(NAME)_$(VERSION)_$(DEB_ARCH).deb
	@echo "package: $(BUILD)/$(NAME)_$(VERSION)_$(DEB_ARCH).deb"

# --- Arch package (requires makepkg; run on an arch host, not as root) ---
arch: release
	rm -rf $(BUILD)/arch
	mkdir -p $(BUILD)/arch
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ \
		-o $(BUILD)/arch/$(NAME)-$(VERSION).tar.gz HEAD
	sed -e 's/@VERSION@/$(VERSION)/' packaging/arch/PKGBUILD.in \
		> $(BUILD)/arch/PKGBUILD
	cd $(BUILD)/arch && makepkg -f
	@echo "package: $(BUILD)/arch/$(NAME)-$(VERSION)-1-*.pkg.tar.*"

# --- RPM package (requires rpmbuild; run on an rpm-based host) ---
rpm: release
	rm -rf $(BUILD)/rpm
	mkdir -p $(BUILD)/rpm/SOURCES $(BUILD)/rpm/SPECS
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ \
		-o $(BUILD)/rpm/SOURCES/$(NAME)-$(VERSION).tar.gz HEAD
	sed -e 's/@VERSION@/$(VERSION)/' packaging/rpm/$(NAME).spec.in \
		> $(BUILD)/rpm/SPECS/$(NAME).spec
	rpmbuild --define "_topdir $(CURDIR)/$(BUILD)/rpm" -bb \
		$(BUILD)/rpm/SPECS/$(NAME).spec
	@echo "package: $(BUILD)/rpm/RPMS/*/$(NAME)-$(VERSION)-1.*.rpm"

clean:
	cargo clean
	rm -rf $(BUILD)
