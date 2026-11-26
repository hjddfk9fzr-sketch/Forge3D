# Forge3D

Forge3D — 3D-движок на Rust. В настольном редакторе можно собрать сцену, настроить физику и игровую логику, проверить игру в Play и экспортировать её в самостоятельный Cargo-проект с исходниками рантайма.

## Что нового в 1.5.0

- События и плавные переходы анимации. В Cloud Relay курьер переключается между покоем, ходьбой и бегом. Шаги звучат по меткам целевого клипа. [Игровая логика](docs/gameplay.md#плавные-переходы-между-клипами)
- Карты нормалей glTF/GLB, свет и отражения от неба, профили теней Desktop и Mobile. [Освещение и материалы](docs/editor.md#освещение-небом-и-профили-теней)
- LOD статических моделей. Детализация базальтовых островов меняется по расстоянию. [Настройка LOD](docs/editor.md#детализация-моделей-по-расстоянию)

## Скачать готовые приложения 1.5.0

| Платформа | Редактор Forge3D | Игра Cloud Relay |
| --- | --- | --- |
| macOS · Apple Silicon | [Forge3D.app · ZIP](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.5.0/Forge3D-1.5.0-macOS-arm64.zip) | [Cloud Relay.app · ZIP](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.5.0/Cloud-Relay-1.5.0-macOS-arm64.zip) |
| Windows · x64 | [Forge3D.exe](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.5.0/Forge3D-1.5.0-Windows-x64.exe) | [Cloud Relay.exe](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.5.0/Cloud-Relay-1.5.0-Windows-x64.exe) |

На Mac распакуйте ZIP и откройте приложение. Сборки имеют ad-hoc подпись без нотарификации Apple. На Windows запустите скачанный EXE. Для готовых приложений Rust устанавливать не требуется. [Описание релиза](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/tag/v1.5.0)

## Первый проект · измените маршрут Cloud Relay

1. Откройте редактор. Выберите **Project → Open Cloud Relay…** и подтвердите открытие демонстрационного проекта. Другой вариант — скачать [редактируемый проект](https://github.com/hjddfk9fzr-sketch/Forge3D/releases/download/v1.5.0/Cloud-Relay.forge-project.json) и открыть его через **File → Open…**.
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
