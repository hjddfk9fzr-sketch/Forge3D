# Игра Forge3D

Это самостоятельный Rust-проект. Исходник точки входа лежит в `src/main.rs`, общий запуск — в `src/lib.rs`, код игры и отрисовки — в `src/runtime.rs`, интеграция Rhai — в `src/scripting.rs`. Стартовая сцена встраивается из `assets/scene.json`. Файл `assets/scenes.json` содержит её ID и остальные сцены проекта вместе с ресурсами.

## Windows и macOS на Apple Silicon

Выполните `cargo run --release` на нужной платформе. Для сборки используйте `cargo build --release`. На Apple Silicon с нативной установкой Rust архитектура по умолчанию — arm64. Её также можно указать явно через `cargo build --release --target aarch64-apple-darwin`.

Команда `cargo run -- --check-scene` проверяет встроенные сцены без окна. Для замера загрузки, времени кадра и памяти запустите `cargo run --release -- --benchmark 180 workshop`, где `workshop` — ID сцены. Уберите ID для замера стартовой сцены. JSON выводится после 30 кадров прогрева и указанного числа измерений. Команда `cargo run -- --window-smoke` открывает окно, отрисовывает восемь кадров и завершает игру.

На Linux для сборки также нужны библиотеки X11, Xi, OpenGL и ALSA (`libx11-dev libxi-dev libgl1-mesa-dev libasound2-dev` в Ubuntu).

## Android ARM64

Установите JDK 17, Gradle 8.11.1, Android SDK Platform 35 и NDK `28.0.13004108`. Задайте `ANDROID_HOME` или укажите путь к SDK в `android/local.properties`. Запустите эти команды из каталога игры.

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
gradle -p android assembleDebug
```

Готовый APK появится в `android/build/outputs/apk/debug/`. Установите его командой `adb install путь-к-apk`. Gradle собирает Rust-библиотеку для arm64 и подключает оболочку Miniquad. Метаданные Android берутся из `android/forge-app.json`. Для релиза настройте `FORGE3D_ANDROID_KEYSTORE`, `FORGE3D_ANDROID_KEY_ALIAS`, `FORGE3D_ANDROID_STORE_PASSWORD` и `FORGE3D_ANDROID_KEY_PASSWORD`. Затем запустите `gradle -p android assembleRelease` для APK либо `gradle -p android bundleRelease` для AAB. Результаты находятся в `android/build/outputs/apk/release/` и `android/build/outputs/bundle/release/` соответственно.

## Управление

Объекты с Movement реагируют на клавиши, назначенные в `bindings`, и на экранные кнопки Android. Логику игры можно продолжить в функции `update` в `src/runtime.rs`.

## Скрипты

Объекты могут содержать поле `script` в `assets/scene.json`. Rhai-скрипт реализует обязательную функцию `update(object, dt, input)` и необязательную `start(object)`. Обе возвращают карту `object`. Она содержит позицию `x/y/z`, поворот в градусах `rx/ry/rz`, масштаб `sx/sy/sz`, видимость `visible` и карту `state`, которая живёт до конца игровой сессии. В `update` доступны секунды кадра `dt` и две оси ввода `input.x` и `input.z`. После изменения сцены пересоберите проект. Чтобы перейти в другую сцену, верните `object.next_scene` с её ID из `start`, `update` или `on_event`. При переходе физика и скрипты запускаются заново.

Общий скрипт хранится в списке `scripts` сцены, а объект ссылается на его индекс через `script_asset`. Поле `parent` содержит постоянный ID родителя. Координаты дочернего объекта и изменения из скрипта применяются относительно этого родителя.

## Интерфейс, звук и прогресс

Поле `presentation` сцены хранит текст, PNG-изображения, кнопки и PCM WAV. Клавиатура, мышь и касания вызывают `on_ui(object, event)`, где `event.id` — ID кнопки. Скрипт может вернуть `object.play_sound`, `object.volume_step`, `object.save_scene` или `object.resume_game`.

Сохранение содержит ID контрольной сцены и громкость. На настольных системах файл `progress.json` лежит в каталоге данных приложения. Переменная `FORGE3D_SAVE_DIRECTORY` указывает другой корневой каталог. На Android используется закрытый каталог приложения. При продолжении сцена загружается с исходной физикой и состоянием объектов.

## Физика

Поле `body` объекта задаёт твёрдое тело Rapier с коллайдером Box или Sphere. Динамические тела располагаются в корне сцены. `gravity` задаёт вектор гравитации. Симуляция выполняется в `src/physics.rs` с шагом 1/60 секунды.

Необязательный обработчик `on_event(object, event)` получает `kind`, `other_id` и `other_name` и возвращает объект. Виды событий — `collision_enter`, `collision_exit`, `trigger_enter`, `trigger_exit`. Скрипт объекта из `camera_controller` может реализовать `camera(view, object, dt, input)` и возвращать положение цели камеры `x/y/z`, углы `yaw/pitch` и расстояние `distance`.

## Свет и анимация

`lighting` задаёт направленный свет и его карту теней. `light`, `surface` и `cast_shadows` на объекте управляют точечным источником, материалом и тенью. Анимированные модели хранят скелет и клипы в `rig`. Поле `animation` объекта задаёт клип, время, скорость, повтор и воспроизведение. Из Rhai доступны `anim_clip`, `anim_time`, `anim_speed`, `anim_playing` и `anim_loop`. Пустое имя клипа возвращает исходную позу.
