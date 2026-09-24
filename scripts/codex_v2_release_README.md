# codex-v2

Ta paczka zawiera gotowy build `codex-v2` dla platformy podanej w nazwie archiwum.

## Instalacja na macOS ARM64

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

## Instalacja na Windows x64

W PowerShellu, z katalogu powstałego po rozpakowaniu, uruchom:

```powershell
py .\install-codex-v2.py
```

Instalator kopiuje paczkę do `%USERPROFILE%\.local\lib\codex-v2\releases\<build>`, tworzy `%USERPROFILE%\.local\bin\codex-v2.cmd` i ustawia dla niego osobny `CODEX_HOME=%USERPROFILE%\.codex-v2`. Przy pierwszej instalacji pyta, czy sklonować stan z `%USERPROFILE%\.codex`.

Dodaj `%USERPROFILE%\.local\bin` do zmiennej użytkownika `PATH`, otwórz nowy PowerShell i sprawdź:

```powershell
codex-v2 --version
codex-v2 login status
```

Jeżeli launcher jest już widoczny w bieżącym `PATH`, możesz odświeżyć rozpoznawanie poleceń przez otwarcie nowego PowerShella. Python można też uruchomić poleceniem `python`, jeśli system nie ma launchera `py`.

Do instalacji bez interaktywnego pytania służy `--clone-state` albo
`--no-clone-state`. Inny katalog źródłowy można wskazać przez `--source-home`.
