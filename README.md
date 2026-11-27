# Forge3D

Forge3D — игровой движок и редактор 2D- и 3D-сцен на Rust. В настольном редакторе можно собрать сцену, настроить физику и игровую логику, проверить игру в Play и экспортировать её в самостоятельный Cargo-проект с исходниками рантайма.

## Что нового в 1.7.0

На стартовом экране выберите **Templates → Afterlight**, затем нажмите **Play** и **Enter**. Стрелки или A/D управляют курьером, пробел запускает прыжок. В проекте два тайловых уровня, четыре кадра анимации персонажа и переходы через двери-триггеры. В **Project → Edit scene** откройте `yard` или `bridge` для рисования. Новый проект можно начать с **Empty 2D** или **Empty 3D**. Свои проекты открывайте в **My projects**. [Руководство по 2D](docs/editor.md#2d-сцены)

В 3D-вьюпорте появились куб ориентации XYZ и панель Select, Move, Rotate и Scale. WASD перемещает редакторскую камеру вдоль взгляда, а правая кнопка мыши управляет свободным обзором.

## Rust-проекты с 1.6.0

У игры может быть собственный Cargo-пакет. Сохраните проект, выберите **Project → Initialize Rust game…** и откройте **Inspector → Gameplay components → Add Component → project.spin**. Пакет `game-code` объявляет поля компонента и его систему. Редактор читает схему из собранной игры, а **Play** запускает её в отдельном окне. Экспорт включает тот же пакет. [Скачать пример Rust-Starter](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.6.0/Rust-Starter.zip) · [Руководство по Rust-проектам](docs/gameplay.md#rust-проект-игры)

## Скачать Forge3D 1.7.0

| Платформа | Редактор Forge3D | Игра Afterlight |
| --- | --- | --- |
| macOS · Apple Silicon | [Forge3D.app · ZIP](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.7.0/Forge3D-1.7.0-macOS-arm64.zip) | [Afterlight.app · ZIP](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.7.0/Afterlight-1.7.0-macOS-arm64.zip) |

Распакуйте ZIP и откройте приложение. Сборки имеют ad-hoc подпись без нотарификации Apple. Для запуска Rust не требуется. [Редактируемый проект Afterlight](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.7.0/Afterlight.forge-project.json) · [Описание релиза](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/tag/v1.7.0)

Готовые Windows x64 сборки доступны в [релизе 1.6.0](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/tag/v1.6.0).

## Первый проект · измените маршрут Cloud Relay

1. Откройте редактор и выберите **Templates → Cloud Relay** на стартовом экране. Другой вариант — скачать [редактируемый проект](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.6.0/Cloud-Relay.forge-project.json) и открыть его через **File → Open…**.
2. Нажмите **Play**, затем **Enter**. Двигайтесь клавишами WASD или стрелками, прыгайте пробелом. На первом островном маршруте пройдите через передатчик, затем через выход.
3. Остановите игру. В **Project → Edit scene** выберите сцену `amber`, в **Hierarchy** — объект **Exit arch**. В **Inspector → Gameplay components → RelayGate** поменяйте **Destination** с `vesper` на `finish`.
4. Сохраните через **Project → Save project…**. Затем выберите сцену `launch` через **Project → Edit scene**, нажмите **Play** и начните новый забег. Выход с первой трассы теперь ведёт сразу к финишу. Чтобы восстановить исходный маршрут, верните `vesper`.

В 1.4.2 Inspector строит поля по описанию компонента на Rust. Через **Add Component** можно, например, добавить объекту `workshop.pickup` и задать расстояние подбора. [Пример своего компонента](docs/gameplay.md#компоненты-в-inspector)

## Руководства

| Задача | Где читать |
| --- | --- |
| Импортировать модель, собрать и осветить сцену, изменить префаб | [Редактор сцен и ресурсов](docs/editor.md) |
| Добавить персонажа, ввод, компонент на Rust или скрипт Rhai | [Игровая логика](docs/gameplay.md) |
| Запустить редактор из исходников, экспортировать игру, собрать Android | [Сборка и экспорт](docs/build.md) |
| Подключить редактор к агенту | [Настройка MCP](MCP.md) |
| Открыть старый проект или проверить версию формата | [Совместимость](COMPATIBILITY.md) |
| Посмотреть результаты тестирования и планы | [Проверки выпуска](RELEASE-CHECKS.md) · [Дорожная карта](ROADMAP.md) |
