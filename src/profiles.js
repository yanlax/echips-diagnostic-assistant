// Профили моделей: какие тесты гонять в автопрогоне и какие пороги считать
// нормой. Профиль модели накладывается поверх `default`. Ключ модели — часть
// названия модели/кода (как в каталоге драйверов): сравнение без учёта
// регистра и знаков препинания, по вхождению в "производитель + модель".
//
// Поля профиля:
//   name              название профиля (показывается в интерфейсе)
//   tests             порядок тестов автопрогона (id категорий из CATS в app.js)
//   stopAtFail        true — остановить автопрогон на первом непройденном тесте
//   batteryMinHealth  минимальный износ-порог: health% батареи, ниже — «не пройден»
//   wifiMinSignal     порог лучшего сигнала Wi-Fi, % (null — не проверять); слабый
//                     сигнал рядом с роутером указывает на антенну/шлейф
//   smartCautionIsFail  «Тревога» SMART (переназначенные секторы и т. п.) считать неисправностью
//   surfaceScanGb     сколько первых ГБ диска сканировать поверхность в автопрогоне
//   surfaceSlowPct    допустимая доля медленных (150–500 мс) блоков при сканировании, %
//   stressStressors   виды нагрузки стресс-теста, если добавить "stress" в tests: cpu, fpu, cache, memory, disk, gpu
//   throttleMinPct    худшая скорость нагрузки, % от базовой, ниже которой стресс-тест не пройден
//   throttleMaxSharePct  допустимая доля времени (%) со скоростью ниже 80% от базовой
//   stressSecs        длительность стресс-теста CPU, если добавить "stress" в tests
//                     (по умолчанию в автопрогон не входит), с
//   (memTestMb убран в 0.35.0: память проверяется вся свободная, размер не задаётся)
//   memPasses         число проходов теста памяти в автопрогоне
//   diskWriteMb       размер проверочного файла теста записи в автопрогоне, МБ (10240; урезается по свободному месту тома)
//   diskReadSampleMb  размер участка теста чтения диска в автопрогоне, МБ (24 участка; 256 → 6 ГБ)
//   diskSlowBlocksMax допустимо медленных (>250 мс) блоков в тестах чтения/записи
//   sensorsProbeSecs  сколько секунд снимать показания датчиков в автопрогоне
//   maxTempC          порог температуры (простой и под нагрузкой), °C
//   diskMaxWearPct    порог износа SSD, %: выше — «не пройден»
//   crashDays         окно поиска сбоев (синие экраны/перезагрузки), дней
//   unexpectedShutdownsMax  сколько внезапных отключений (Kernel-Power 41 без
//                     синего экрана) допустимо в окне crashDays; синие экраны — всегда ошибка
//   required          категории, обязательные на ноутбуке: если узла нет —
//                     «не пройден»; на десктопе отсутствие = «не применимо»
//   expect            эталон железа для теста «Системная информация»:
//       cpu           подстрока в названии процессора ("i5-1235U")
//       ramGb         объём ОЗУ, ГБ (допуск 10%)
//       diskGb        объём основного диска, ГБ (допуск 10%)
//       biosContains  подстрока в версии BIOS
window.ECHIPS_PROFILES = {
  default: {
    name: "Стандартный",
    tests: ["sys", "winact", "drv", "disk", "smart", "crash", "usb", "bt", "wifi", "lan", "kb", "lcd", "bright", "cam", "pad", "fp", "bat", "snd",
            "diskread", "surface", "diskwrite", "mem", "sens", "fans"],
    stopAtFail: false,
    batteryMinHealth: 80,
    wifiMinSignal: null,
    diskMaxWearPct: 90,
    smartCautionIsFail: true,
    surfaceScanGb: 20,
    surfaceSlowPct: 1,
    stressStressors: ["cpu", "fpu"],
    stressSecs: 60,
    throttleMinPct: 60,
    throttleMaxSharePct: 20,
    memPasses: 2,
    diskWriteMb: 10240,
    diskReadSampleMb: 256,
    diskSlowBlocksMax: 3,
    sensorsProbeSecs: 8,
    maxTempC: 95,
    crashDays: 30,
    unexpectedShutdownsMax: 2,
    required: ["wifi", "bt", "bat"],
    expect: {}
  },
  models: {
    // Пример (значения подставьте по реальной спецификации модели):
    // "NB156D": {
    //   name: "Taganay NB156D",
    //   batteryMinHealth: 85,
    //   expect: { cpu: "i5", ramGb: 8, diskGb: 256 }
    // }
  }
};
