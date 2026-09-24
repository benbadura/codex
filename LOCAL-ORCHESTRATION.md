# Lokalny `codex-v2`: konfiguracja, prompty i instalacja

Instrukcja dla zmian w tym checkoutcie, zweryfikowana 23.09.2026 na macOS Apple Silicon. Nowe parametry nie są dostępne automatycznie w CLI instalowanym przez npm lub Homebrew. Trzeba uruchomić build zawierający tę poprawkę.

## 1. Co oznacza nowy i stary flow

Oba poniższe flow korzystają z **MultiAgentV2**. „Stary” oznacza tutaj dotychczasowe oczekiwanie z timeoutem, a nie starsze API MultiAgentV1.

| Zachowanie | Nowy flow: zdarzenia | Stary flow: timeouty |
| --- | --- | --- |
| Wywołanie przez model | `wait_agent({"mode":"until_event"})` | `wait_agent({"timeout_ms":60000})` |
| Upływ czasu | Nie kończy oczekiwania | Kończy oczekiwanie; model może ponowić wywołanie |
| Raport postępu | `send_message` z `kind: "progress"` | Zwykły `send_message`, jeśli raport ma dotrzeć do modelu |
| Wynik, błąd, pytanie lub blokada | Wybudza przez istniejący kanał wiadomości | Również może zakończyć oczekiwanie wcześniej |
| Nowa instrukcja użytkownika | Przerywa oczekiwanie | Przerywa oczekiwanie |

`until_event` korzysta z oczekiwania w runtime, bez cyklicznego oddawania sterowania modelowi. Zwykłe wiadomości nadal wybudzają oczekiwanie — runtime nie rozpoznaje semantycznie, czy tekst jest „tylko statusem”. To nadawca wybiera `kind: "progress"`.

Raport `progress` pokazuje odbiorcy sygnał aktywności, np. `Progress from /root/worker`. **Treść raportu nie trafia ani do modelu odbiorcy, ani do tego wpisu UI**; zostaje w historii wywołania narzędzia u nadawcy. Raport nie uruchamia nowej tury nieaktywnego odbiorcy.

Istotne ograniczenia:

- Nie łącz `mode: "until_event"` z `timeout_ms` — takie wywołanie zwraca błąd.
- Gdy nie ma innych aktywnych agentów ani oczekującego wejścia, narzędzie zwraca `no_active_agents`. Orkiestrator powinien wtedy podsumować wynik albo zaplanować dalszą pracę, zamiast ponawiać pustą pętlę.
- Pominięcie `mode` zachowuje stary tryb timeoutu. Można też podać `mode: "timeout"`.
- Poprawka nie wymusza wszystkich decyzji modelu: instrukcje kierują go do nowego flow, ale nie blokują ręcznego użycia timeoutów czy `list_agents`.
- Nie dodaje automatycznej zmiany modelu na tańszy, trwałego harmonogramu po restarcie ani watchdogu zawieszonych agentów.

**Ultra jest osobną opcją.** `model_reasoning_effort = "ultra"` wybiera poziom rozumowania i w MultiAgentV2 domyślnie sprzyja proaktywnej delegacji. `until_event` działa także przy innym poziomie rozumowania, jeśli agent używa narzędzi MultiAgentV2. Nie ma nowego przełącznika `/ultra-event`.

## 2. Instalacja pod nazwą `codex-v2`

Układ instalacji:

```text
~/.local/bin/codex-v2                     # wrapper dostępny w PATH
~/.local/lib/codex-v2/releases/<build>/   # cały pakiet z helperami
~/.local/lib/codex-v2/current             # symlink do wybranego buildu
~/.codex-v2/                             # osobna konfiguracja, auth i sesje
```

Zwykłe `codex` zachowuje swoją nazwę i instalację. Wrapper ustawia własny `CODEX_HOME` tylko dla uruchamianego procesu oraz używa `--no-daemon`, aby testowana wersja nie podłączała się do wspólnego serwera starego CLI. Pliki projektu i jego lokalna konfiguracja `.codex/` pozostają wspólne, jeśli oba programy uruchomisz w tym samym repozytorium.

### Zalecane: gotowa paczka z GitHub Release

Workflow [codex-v2-release.yml](.github/workflows/codex-v2-release.yml) uruchamia się po utworzeniu release'a. Buduje i testuje paczki dla macOS ARM64/Intel oraz Linux ARM64/x64, generuje `codex-v2_SHA256SUMS` i dopina wszystko do tego samego release'a. Użyj tagu w formacie `codex-v2-vX.Y.Z`, np. `codex-v2-v0.1.0`; tag ma wskazywać kod, który chcesz zbudować.

Na tym Macu wybierz `aarch64-apple-darwin`. Pobierz z release'a archiwum `codex-v2-<wersja>-aarch64-apple-darwin.tar.gz` i plik sum, a następnie:

```bash
CODEX_V2_ARCHIVE="$(find . -maxdepth 1 -name 'codex-v2-*-aarch64-apple-darwin.tar.gz' -print -quit)"
grep "$(basename "$CODEX_V2_ARCHIVE")" codex-v2_SHA256SUMS | shasum -a 256 -c -
tar -xzf "$CODEX_V2_ARCHIVE"
cd codex-v2
python3 install-codex-v2.py
```

Instalator kopiuje paczkę do wersjonowanego katalogu pod `~/.local/lib/codex-v2/releases`, więc po zakończeniu można usunąć pobrane archiwum i katalog po rozpakowaniu. Aktualizacja polega na pobraniu nowego release'a i ponownym uruchomieniu jego instalatora; stan w `~/.codex-v2` zostaje zachowany.

Workflow musi znajdować się na domyślnej gałęzi przed utworzeniem release'a. Trigger `created` nie działa dla draftów. Jeśli release utworzył inny workflow za pomocą jego `GITHUB_TOKEN` albo chcesz ponowić nieudany build, uruchom ręcznie workflow **codex-v2 release packages** i podaj istniejący tag. Ręczne uruchomienie nadpisuje aktywa o tych samych nazwach.

### Alternatywa: lokalny build

Poniższe kroki są potrzebne tylko wtedy, gdy chcesz zainstalować niezacommitowane zmiany albo nie masz jeszcze gotowego release'a.

### Krok 1: przygotuj narzędzia

Na tym komputerze narzędzia Rust i `just` zostały już przygotowane podczas implementacji. Przy ponownej instalacji środowiska, zakładając dostępny Homebrew:

```bash
xcode-select -p
# Jeśli brakuje Command Line Tools:
# xcode-select --install

brew install rustup just
export PATH="$(brew --prefix rustup)/bin:$PATH"

cd /Users/beniaminbadura/Documents/repos/codex
rustup toolchain install 1.95.0 --component clippy --component rustfmt --component rust-src
python3 --version
```

Builder wymaga Pythona 3.11 lub nowszego. Przypięta wersja Rusta jest w `codex-rs/rust-toolchain.toml`; po późniejszej aktualizacji repozytorium sprawdź ten plik ponownie. Nie trzeba ustawiać globalnego domyślnego toolchainu.

### Krok 2: zbuduj kompletny pakiet

Wykonuj kolejne bloki w tym samym terminalu. Builder uwzględnia bieżące, również niezacommitowane zmiany checkoutu.

```bash
cd /Users/beniaminbadura/Documents/repos/codex
export PATH="$(brew --prefix rustup)/bin:$PATH"

EVENT_BUILD_ID="$(date +%Y%m%d-%H%M%S)"
EVENT_PACKAGE="$HOME/.local/lib/codex-v2/releases/$EVENT_BUILD_ID"

SSL_CERT_FILE=/etc/ssl/cert.pem just assemble-codex-package \
  --cargo-profile dev-small \
  --package-dir "$EVENT_PACKAGE"
```

Pierwsza kompilacja może potrwać i zająć dużo miejsca. `dev-small` jest domyślnym profilem lokalnego buildera; do późniejszego buildu zoptymalizowanego można wybrać `--cargo-profile release`.

Builder dobiera target hosta, dołącza `codex-code-mode-host`, ripgrep i odpowiednie zasoby zsh. Pobiera również zweryfikowaną parę archiwum/bindingów V8 dla wersji przypiętej w repozytorium. To istotne: podczas implementacji domyślny publiczny URL V8 zwracał 404, a repozytorium ma własne artefakty. Ustawienie `SSL_CERT_FILE` w tym przykładzie rozwiązuje problem certyfikatów napotkany w tutejszym Pythonie na macOS.

**Szybki wariant, jeżeli masz aktualne binarki debug:** zamiast polecenia powyżej możesz spakować je bez ponownej kompilacji. Starsze binarki z implementacji event wait nie zawierają nowego panelu zużycia — po zmianie źródeł użyj normalnego buildera:

```bash
SSL_CERT_FILE=/etc/ssl/cert.pem just assemble-codex-package \
  --entrypoint-bin "$PWD/codex-rs/target/debug/codex" \
  --code-mode-host-bin "$PWD/codex-rs/target/debug/codex-code-mode-host" \
  --package-dir "$EVENT_PACKAGE"
```

Wybierz jeden wariant. Flagi `--entrypoint-bin` i `--code-mode-host-bin` **nie przebudowują kodu** — po kolejnych zmianach źródeł użyj normalnego buildera. Nie przenoś samego `bin/codex` poza katalog pakietu, ponieważ musi odnajdywać zasoby i helpery.

### Krok 3: wybierz build i utwórz wrapper

```bash
python3 scripts/install_codex_v2.py --package-dir "$EVENT_PACKAGE"

export PATH="$HOME/.local/bin:$PATH"
rehash
codex-v2 --version
command -v codex-v2
command -v codex
```

Przy pierwszej instalacji instalator zapyta:

```text
Sklonować stan oryginalnego Codexa z /Users/twoj-login/.codex? [t/N]:
```

- **`t` / `tak`** — kopiuje stan do `~/.codex-v2`: konfigurację, profile, historię, sesje, zwykłe katalogi skills/pluginów i plik `auth.json`, jeśli istnieje. Źródłowy katalog pozostaje na miejscu. Kopiowane pliki nie są dowiązaniami do oryginału.
- **Enter / `n`** — tworzy pusty katalog dla nowej instalacji.
- Jeśli `~/.codex-v2` już istnieje, aktualizacja zachowuje jego stan i pomija pytanie. Jawne `--clone-state` wtedy kończy się błędem; instalator nie scala ani nie nadpisuje istniejącego stanu, nawet pustego katalogu.

Przed klonowaniem zamknij sesje CLI, aplikację i daemon używające źródłowego katalogu. SQLite jest kopiowany przez backup API (łącznie z zatwierdzonymi danymi WAL), ale cała kopia katalogu nie jest atomowym snapshotem działającej aplikacji. Indeks sesji otrzymuje ścieżki do skopiowanych rolloutów. Instalator pomija katalogi runtime `tmp`, `locks`, `daemon`, pliki blokad, sockety oraz dowiązania symboliczne i wypisuje pominięte ścieżki. Dowiązane skills/pluginy trzeba dodać osobno.

Kopia nie przenosi wpisów systemowego keychaina ani danych spoza katalogu źródłowego. Absolutne ścieżki w konfiguracji (np. własne `sqlite_home`, ścieżki MCP/pluginów) pozostają bez zmian — dostosuj je w kopii przed uruchomieniem, jeśli wskazują stan oryginalnej instalacji. To jednorazowa kopia, nie synchronizacja.

Do instalacji bez pytań wybierz jawnie jedną opcję (bez terminala i przy istniejącym źródle instalator wymaga wyboru):

```bash
python3 scripts/install_codex_v2.py --package-dir "$EVENT_PACKAGE" --clone-state
# albo:
python3 scripts/install_codex_v2.py --package-dir "$EVENT_PACKAGE" --no-clone-state
```

Jeśli oryginalny Codex używa innego katalogu niż `~/.codex`, dodaj `--source-home /sciezka/do/stanu`. Instalator celowo nie wybiera źródła z odziedziczonego `CODEX_HOME`.

Wrapper jest nazwany `codex-v2`, ale wewnętrzna binarka i jej komunikat `--version` nadal mogą mówić `codex` lub `codex-cli`. To oczekiwane.

Żeby nazwa była dostępna w nowych terminalach zsh, dodaj jednokrotnie do `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Krok 4: dodaj konfigurację i zaloguj się

Przy czystej instalacji utwórz `~/.codex-v2/config.toml` z konfiguracją z następnej sekcji. Po klonowaniu uzupełnij istniejący plik o te ustawienia, zachowując swój provider, MCP i pozostałe opcje. Sprawdź logowanie:

```bash
codex-v2 login status
# Jeśli brak aktywnego logowania (np. dane były tylko w keychainie):
# codex-v2 login
codex-v2 features list
```

Osobny `CODEX_HOME` oznacza osobne pliki ustawień MCP, pluginów i uwierzytelnienia. Po klonowaniu mogą już zawierać Twoją konfigurację; bez klonowania skonfiguruj je osobno. Zewnętrzne usługi, repozytoria i systemowy keychain nie są klonowane.

### Krok 5: uruchom pracę

```bash
codex-v2 --strict-config -C /sciezka/do/projektu
```

Wklej prompt z sekcji 4. Przy porównaniu flow zaczynaj nową sesję; wznowiona historia może zawierać wcześniejsze instrukcje i wywołania narzędzi.

## 3. Konfiguracja nowego i starego flow

### Domyślny nowy flow

Zapisz w `~/.codex-v2/config.toml`:

```toml
model = "gpt-6-astra"
model_reasoning_effort = "ultra"

[features.multi_agent_v2]
enabled = true
wait_agent_enabled = true
expose_spawn_agent_model_overrides = true
max_concurrent_threads_per_session = 4
```

Limit `4` obejmuje orkiestratora, więc pozostają maksymalnie trzy równocześnie aktywne subagenty. `expose_spawn_agent_model_overrides` udostępnia wybór modelu przy delegacji; nie ustawia automatycznie Luny.

Identyfikatory `gpt-6-astra` oraz używany niżej `gpt-5.6-luna` występują w lokalnym katalogu modeli. Dostępność zależy od konta i providera; sprawdź `/model` w uruchomionym CLI. Jeśli nie masz danego modelu, wybierz dostępny identyfikator i popraw również prompt. Ta instrukcja nie potwierdza cen ani dostępności dla konta.

Nie ma klucza TOML `wait_mode` ani `event_driven = true`. Tryb wybiera model parametrem konkretnego `wait_agent`; domyślne instrukcje tej poprawki zalecają `until_event`.

**Uwaga przy własnych instrukcjach:** `root_agent_usage_hint_text` i `subagent_usage_hint_text` zastępują instrukcje odpowiedniej roli, zamiast je rozszerzać. Jeśli je ustawisz, nowa domyślna wskazówka o oczekiwaniu nie zostanie do tej roli automatycznie dopisana. Uwzględnij wtedy zasady `until_event`, `progress` i wyboru modeli we własnym tekście.

### Profil starego flow do porównania

W tym checkoutcie `-p nazwa` wczytuje plik `$CODEX_HOME/nazwa.config.toml`. Utwórz **`~/.codex-v2/timeout.config.toml`**, bez otaczającej tabeli `[profiles.timeout]`:

```toml
[features.multi_agent_v2]
enabled = true
wait_agent_enabled = true
min_wait_timeout_ms = 10000
default_wait_timeout_ms = 60000
max_wait_timeout_ms = 3600000

root_agent_usage_hint_text = """
Koordynuj agentów zgodnie z zadaniem użytkownika. Deleguj niezależne zadania,
podawaj zakres plików i kryteria odbioru. Agenci współdzielą katalog roboczy.
Gdy czekasz, używaj wait_agent z mode="timeout" i timeout_ms=60000.
Po timeoutcie oceń sytuację i w razie potrzeby poczekaj ponownie.
Nie używaj mode="until_event" w tym porównaniu. list_agents służy inspekcji.
Używaj send_message bez kind="progress", jeśli wiadomość ma dotrzeć do modelu.
Wybieraj model wykonawcy zgodnie z wyraźną instrukcją użytkownika;
przy innym modelu używaj fork_turns="none" albo ograniczonej liczby tur.
"""

subagent_usage_hint_text = """
Wykonaj przydzielone zadanie, przestrzegając zakresu plików.
Nie cofaj zmian innych agentów. Raporty i blokady wysyłaj przez send_message
bez kind="progress". Na końcu podaj wynik, pliki i przeprowadzone sprawdzenia.
"""
```

Uruchomienie:

```bash
codex-v2 -p timeout -C /sciezka/do/projektu
```

Bez profilu wracasz do nowego zalecanego flow. W niezmienionej konfiguracji timeouty wynoszą: minimum 10 s, domyślnie 30 s, maksimum 1 h. Krótsza wartość jest podnoszona do minimum, a przekroczenie maksimum jest błędem. Te ustawienia nie nakładają limitu na `until_event`.

Wyłączenie `features.multi_agent_v2` nie jest przełącznikiem „starego oczekiwania”: wybór wersji narzędzi może nadal zależeć od katalogu modelu. Do porównania używaj tego samego API V2 i profilu powyżej.

## 4. Jak promptować

### Nowy flow: drogi orkiestrator i Luna jako wykonawca

Wklej na początku nowej sesji, uzupełniając zadanie:

```text
Zadanie: [opisz konkretny wynik i kryteria odbioru].

Deleguj niezależne części do gpt-5.6-luna z reasoning_effort="medium"
i fork_turns="none". Wyraźnie zezwalam na ten wybór modelu wykonawców.
Maksymalnie trzy subagenty równocześnie. Ty odpowiadasz za podział pracy,
decyzje, integrację i końcową weryfikację.

Każdemu wykonawcy przekaż samodzielny brief: cel, zakres plików,
ograniczenia, niezbędny kontekst i kryteria odbioru. Uprzedź go,
że inni agenci mogą edytować repozytorium i nie wolno cofać ich zmian.

Pracuj na niezależnej części zadania, dopóki masz taką możliwość.
Gdy czekasz na wykonawców, używaj wait_agent z mode="until_event",
bez timeout_ms. Nie sprawdzaj cyklicznie list_agents.

Przekaż wykonawcom: zwykły postęp sygnalizuj przez send_message
z kind="progress". Pytania, blokady i decyzje wysyłaj zwykłym
send_message. Zakończ pracę końcowym raportem: wynik, pliki, testy,
pozostałe problemy. Nie wysyłaj tego samego raportu wielokrotnie.

Po no_active_agents oceń wyniki i dalsze kroki; nie ponawiaj pustego czekania.
Jeśli Luna nie jest dostępna, zgłoś to zamiast bez pytania uruchamiać
wykonawców na droższym modelu.
```

`fork_turns: "none"` ogranicza dziedziczenie konwersacji, dlatego brief musi być samodzielny. Domyślne `"all"` dziedziczy model i poziom rozumowania rodzica oraz nie pozwala ich nadpisać. Nazwy narzędzi w przykładach są wskazówkami dla modelu — nie poleceniami do wpisania w shellu.

Przykładowe argumenty wywołań agenta:

```json
{"task_name":"tests","model":"gpt-5.6-luna","reasoning_effort":"medium","fork_turns":"none","message":"Samodzielny brief z zakresem plików i kryteriami odbioru."}
```

```json
{"target":"/root","kind":"progress","message":"Uruchamiam testy modułu."}
```

```json
{"target":"/root","message":"Blokada: potrzebuję decyzji, czy zachować zgodność starego formatu."}
```

### Stary flow: kontrolowane porównanie

Uruchom profil `timeout` i użyj tego samego zadania oraz tych samych modeli. W prompcie zmień zasady oczekiwania na:

```text
W tym porównaniu używaj starego flow: wait_agent z mode="timeout"
i timeout_ms=60000. Nie używaj until_event. Gdy timeout minie,
oceń sytuację i poczekaj ponownie, jeśli zadanie nadal trwa.
Raporty wykonawców, które mają dotrzeć do orkiestratora,
wysyłaj zwykłym send_message, bez kind="progress".
```

Jeśli chcesz porównać wyłącznie wpływ timeoutów, w obu przebiegach nie wysyłaj okresowych raportów. Jeśli jednocześnie zmienisz model, brief i częstotliwość wiadomości, różnica zużycia nie będzie miarą samego oczekiwania.

## 5. Jak sprawdzić efekt i utrzymywać instalację

### Podgląd zużycia: F5 i `/detailed-status`

1. Uruchom świeżą sesję `codex-v2` i zleć zadanie. Liczniki zbierają się od początku działania TUI, również przy zamkniętym panelu.
2. Naciśnij **F5**, aby otworzyć live podgląd. Na Macu może być potrzebne **Fn+F5**. F3 pozostaje skrótem wyszukiwania. Jeśli własna konfiguracja zajmuje F5 (także jako początek skrótu wieloklawiszowego), ma pierwszeństwo; użyj wtedy komendy z następnego punktu.
3. Wpisz **`/detailed-status`**, aby otworzyć szczegółowy panel. Komenda działa również podczas pracy agenta i nie wysyła promptu do modelu.
4. Przewijaj strzałkami lub Page Up / Page Down. Zamknij panel **F5**, **Esc** albo **q** przy domyślnych skrótach.

Podgląd pokazuje parenta, sumę subagentów (również zagnieżdżonych), sumę całej sesji oraz podział na modele. Szczegóły dodają nazwy i identyfikatory agentów, ich parentów, status, bieżący model, zużycie według modelu, liczbę obserwowanych tur i łączny czas tych tur. Wyniki zakończonych i zamkniętych agentów pozostają w zestawieniu do zamknięcia TUI. Przełączenie na subagenta zachowuje zakres całej jego sesji.

„Koszt” oznacza tu **tokeny**, nie kwotę w USD ani procent limitu abonamentu. `input` obejmuje `cached`, a `output` obejmuje `reasoning`; suma to `input + output`. Liczniki aktualizują się po odpowiedzi modelu, kiedy serwer zgłasza zużycie. Czas obejmuje narzędzia i oczekiwanie, nie tylko inferencję. Sam panel odświeża ekran lokalnie, bez dodatkowych zapytań do modeli i bez pollingu subagentów. Dla nowo wykrytego wątku, którego metadanych jeszcze nie zna, wykonuje jednorazowy odczyt z app-servera, aby ustalić parenta i model.

Zakres pomiaru: panel korzysta z wątków i zdarzeń obserwowanych przez bieżące TUI. Po wznowieniu sesji nie odtwarza kompletnego historycznego drzewa agentów ani ich czasów pracy. Wcześniejsze tokeny bez pewnego przypisania do modelu trafiają do `unknown / before observation`. Nieudany odczyt metadanych, przerwa w zdarzeniach, cofnięcie historii lub osiągnięcie limitu śledzenia oznacza dane jako `Partial data`. Brak zgłoszonego zużycia nie jest dowodem zerowego kosztu.

Do porównania starego i nowego flow uruchom dwa świeże wątki z tym samym zadaniem i modelami. Po zakończeniu sprawdź `/detailed-status`, szczególnie `Parent`, jego `input`, `cached` i `output`, oraz osobno sumę `Subagents`. Zapisz wynik przed zamknięciem TUI; zestawienie nie jest osobnym trwałym raportem.

### Kontrola zachowania i aktualizacja

Przy nowym flow powinno być widoczne jedno oczekiwanie `until_event` trwające aż do istotnego zdarzenia, zamiast kolejnych powrotów `Wait timed out.`. Raport `progress` może pojawić się w UI bez wznowienia orkiestratora. Poproś wykonawcę o przesłanie prawdziwej blokady: ta wiadomość powinna go wybudzić.

Porównuj liczbę wywołań i zużycie **orkiestratora osobno od wykonawców**, uwzględniając cache, jeśli statystyki providera go pokazują. Brak cyklicznych wybudzeń nie oznacza zerowego kosztu całego zadania: nadal kosztują planowanie, praca wykonawców, odpowiedzi na blokady i końcowa synteza.

Aktualizacja lokalnego forka: zbuduj nowy katalog `releases/<build>` krokiem 2, sprawdź `"$EVENT_PACKAGE/bin/codex" --version`, a potem przestaw `current` poleceniem z kroku 3. Zamknij i uruchom ponownie lokalne sesje, aby korzystały z nowego buildu. Powrót do poprzedniej wersji polega na przestawieniu `current` na poprzedni katalog.

Nie używaj `codex-v2 update` do utrzymywania tej poprawki: standardowy updater nie buduje Twojego zmodyfikowanego checkoutu. Zwykły `codex` aktualizuj jego dotychczasową metodą.

Aby wycofać dodatkowe CLI z użycia, usuń wyłącznie wrapper `~/.local/bin/codex-v2`. Pakiety i `~/.codex-v2` można zachować do późniejszego powrotu; usunięcie tego drugiego katalogu usuwa również lokalne sesje i ustawienia tej instalacji.

## Źródła w tym checkoutcie

- [Implementacja oczekiwania](codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs) i [parametry narzędzi](codex-rs/core/src/tools/handlers/multi_agents_spec.rs).
- [Konfiguracja MultiAgentV2](codex-rs/features/src/feature_configs.rs), [domyślne instrukcje](codex-rs/prompts/src/multi_agent_instructions.rs), [wybór instrukcji roli i trybu Ultra](codex-rs/core/src/session/multi_agents.rs).
- [Builder kompletnego pakietu](scripts/codex_package/README.md) i [artefakty V8](third_party/v8/README.md).
- [Testy zachowania event wait](codex-rs/core/tests/suite/agent_event_wait_tests.rs).
