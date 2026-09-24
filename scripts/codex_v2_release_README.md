# codex-v2

Ta paczka zawiera gotowy build `codex-v2` dla platformy podanej w nazwie archiwum.

## Instalacja

Wymagany jest Python 3.10 lub nowszy. Z katalogu powstałego po rozpakowaniu uruchom:

```bash
python3 install-codex-v2.py
```

Instalator:

- kopiuje tę paczkę do `~/.local/lib/codex-v2/releases/<build>`;
- ustawia `~/.local/lib/codex-v2/current` na wybrany build;
- tworzy launcher `~/.local/bin/codex-v2` z osobnym `CODEX_HOME=~/.codex-v2`;
- przy pierwszej instalacji pyta, czy sklonować stan z `~/.codex`.

Po instalacji możesz usunąć archiwum i katalog po rozpakowaniu — zainstalowany build jest niezależną kopią.
Jeśli `~/.local/bin` nie znajduje się w `PATH`, dodaj do `~/.zshrc` albo `~/.bashrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Następnie otwórz nowy terminal i sprawdź:

```bash
codex-v2 --version
codex-v2 login status
```

Do instalacji bez interaktywnego pytania służy `--clone-state` albo
`--no-clone-state`. Inny katalog źródłowy można wskazać przez `--source-home`.
