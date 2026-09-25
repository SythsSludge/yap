# Build and install yap without a package manager:
#
#   make
#   sudo make install              # into /usr/local
#   make install PREFIX=~/.local   # just for you, no sudo
#
# Installs the `yap` command and `yiff` as a second name for it.
#
# Packagers: `make install DESTDIR=$pkgdir PREFIX=/usr`.

PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo
CARGOFLAGS ?= --locked

BIN := target/release/yap
DIST := target/dist
ROOT := $(DESTDIR)$(PREFIX)

.PHONY: all build dist install uninstall clean

all: build

# Always ask cargo; it only rebuilds what changed.
build:
	$(CARGO) build --release $(CARGOFLAGS)

# Completions and the man page, generated from the command-line definition.
dist: build
	mkdir -p $(DIST)
	$(BIN) completions bash > $(DIST)/yap.bash
	$(BIN) completions zsh > $(DIST)/_yap
	$(BIN) completions fish > $(DIST)/yap.fish
	$(BIN) completions bash --bin yiff > $(DIST)/yiff.bash
	$(BIN) completions zsh --bin yiff > $(DIST)/_yiff
	$(BIN) completions fish --bin yiff > $(DIST)/yiff.fish
	$(BIN) manpage > $(DIST)/yap.1

install: dist
	install -Dm755 $(BIN) $(ROOT)/bin/yap
	ln -sf yap $(ROOT)/bin/yiff
	install -Dm644 $(DIST)/yap.bash $(ROOT)/share/bash-completion/completions/yap
	install -Dm644 $(DIST)/_yap $(ROOT)/share/zsh/site-functions/_yap
	install -Dm644 $(DIST)/yap.fish $(ROOT)/share/fish/vendor_completions.d/yap.fish
	install -Dm644 $(DIST)/yiff.bash $(ROOT)/share/bash-completion/completions/yiff
	install -Dm644 $(DIST)/_yiff $(ROOT)/share/zsh/site-functions/_yiff
	install -Dm644 $(DIST)/yiff.fish $(ROOT)/share/fish/vendor_completions.d/yiff.fish
	install -Dm644 $(DIST)/yap.1 $(ROOT)/share/man/man1/yap.1
	ln -sf yap.1 $(ROOT)/share/man/man1/yiff.1
	install -Dm644 assets/yap.desktop $(ROOT)/share/applications/yap.desktop
	install -Dm644 LICENSE $(ROOT)/share/licenses/yap/LICENSE
	install -Dm644 THIRD_PARTY_NOTICES.md $(ROOT)/share/licenses/yap/THIRD_PARTY_NOTICES.md
	install -Dm644 assets/dictionaries/en_US.LICENSE $(ROOT)/share/licenses/yap/en_US-dictionary.LICENSE
	install -Dm644 README.md $(ROOT)/share/doc/yap/README.md

uninstall:
	rm -f $(ROOT)/bin/yap $(ROOT)/bin/yiff \
		$(ROOT)/share/bash-completion/completions/yap $(ROOT)/share/bash-completion/completions/yiff \
		$(ROOT)/share/zsh/site-functions/_yap $(ROOT)/share/zsh/site-functions/_yiff \
		$(ROOT)/share/fish/vendor_completions.d/yap.fish $(ROOT)/share/fish/vendor_completions.d/yiff.fish \
		$(ROOT)/share/man/man1/yap.1 $(ROOT)/share/man/man1/yiff.1 \
		$(ROOT)/share/applications/yap.desktop
	rm -rf $(ROOT)/share/licenses/yap $(ROOT)/share/doc/yap

clean:
	$(CARGO) clean
