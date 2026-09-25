/* Echips Hardware Check — интеграция дизайн-прототипа с реальным Tauri-бэкендом.
   Вёрстка экранов (screenX/fieldX функции) перенесена из дизайна почти
   дословно — визуально ничего не должно отличаться. Изменена внутренняя
   логика: где есть реальные данные (устройство, батарея, USB/BT/Wi-Fi,
   отпечаток, драйверы, замена платы, отчёт) — используется invoke() в Rust;
   где реальных данных физически нет без стороннего софта (датчики) или
   тест принципиально делается глазами техника (клавиатура/дисплей/тачпад) —
   оставлено как в прототипе, с пометкой в интерфейсе. */
(function () {
'use strict';

var _rawInvoke = window.__TAURI__.core.invoke;
var tauriEvent = window.__TAURI__.event;
var getCurrentWindow = window.__TAURI__.window.getCurrentWindow;

/* Обёртка invoke() — пишет каждый вызов в S.adminLog (кольцевой буфер) для
   админ-панели (Shift+F10, только для инженера с role==='admin', см.
   isAdmin/renderAdminPanel ниже). Сама логика вызовов не меняется — просто
   наблюдение сбоку, поведение invoke() для остального кода идентично. */
function invoke(cmd, args){
  var entry = { t:Date.now(), cmd:cmd, args:args, state:'pending' };
  S.adminLog.push(entry);
  if (S.adminLog.length > 300) S.adminLog.shift();
  if (S.adminPanelOpen) renderAdminPanel();
  var p = _rawInvoke(cmd, args);
  p.then(function(res){ entry.state='ok'; entry.result=res; if (S.adminPanelOpen) renderAdminPanel(); },
         function(err){
           entry.state='err'; entry.error=err; if (S.adminPanelOpen) renderAdminPanel();
           // сессия на сервере истекла — просим ввести PIN заново (данные и результаты остаются)
           if (typeof err==='string' && /Сессия недействительна|Нет входа на сервере/.test(err)) relockSession();
         });
  return p;
}

var CATS = [
  { id:'sys', tag:'SYS', name:'Системная информация', method:'Процессор, ОЗУ, диски, видеокарта, плата, BIOS + сверка с профилем модели', impl:'реальные данные', kind:'runner', fetch:'sys' },
  { id:'ident', tag:'ID', name:'Идентификаторы', method:'Сверка SN системы, SN платы, UUID, MAC и OEM-ключа Windows — после замены платы', impl:'реальные данные', kind:'runner', fetch:'ident' },
  { id:'winact', tag:'WIN', name:'Активация Windows', method:'Статус лицензии и канал, ключ OEM в BIOS; устранение: онлайн-активация, ключ OEM, служба, время', impl:'реальные данные', kind:'runner', fetch:'winact' },
  { id:'drv', tag:'DRV', name:'Драйверы', method:'Устройства без драйвера в Диспетчере устройств — только проверка, установка на вкладке «Установка драйверов»', impl:'реальные данные', kind:'runner', fetch:'drv' },
  { id:'disk', group:'disk', sub:'Здоровье', tag:'HDD', name:'Диск: здоровье', method:'Состояние, износ, температура и ошибки (Get-PhysicalDisk, счётчики надёжности)', impl:'реальные данные', kind:'runner', fetch:'disk' },
  { id:'smart', group:'disk', sub:'SMART', tag:'SMART', name:'Диск: SMART', method:'Атрибуты SMART (SATA) и лог здоровья NVMe: износ, температура, ошибки — как в CrystalDiskInfo', impl:'реальные данные', kind:'smart' },
  { id:'crash', tag:'BSOD', name:'Журнал сбоев', method:'Синие экраны и внезапные перезагрузки: события Windows + minidump', impl:'реальные данные', kind:'runner', fetch:'crash' },
  { id:'usb', tag:'USB', name:'USB-порты', method:'Список устройств на USB-шине (WMI PnP) + статус', impl:'реальные данные', kind:'runner', fetch:'usb' },
  { id:'rem', tag:'FLASH', name:'Накопитель USB', method:'Запись и чтение флешки на порту с проверкой данных и замером скорости', impl:'реальная нагрузка', kind:'removable', interactive:true },
  { id:'bt', tag:'BT', name:'Bluetooth', method:'Статус адаптера и список сопряжённых устройств', impl:'реальные данные', kind:'runner', fetch:'bt' },
  { id:'wifi', tag:'WIFI', name:'Wi-Fi', method:'Адаптер + список видимых сетей (netsh wlan)', impl:'реальные данные', kind:'runner', fetch:'wifi' },
  { id:'lan', tag:'LAN', name:'LAN (Ethernet)', method:'Адаптер, состояние линка и скорость', impl:'реальные данные', kind:'runner', fetch:'lan' },
  { id:'kb', tag:'KEY', name:'Клавиатура', method:'Карта клавиш, детект n-key rollover; залипы — глазами', impl:'интерактивно', kind:'keyboard', interactive:true },
  { id:'lcd', tag:'LCD', name:'Матрица', method:'Заливка сплошными цветами — битые пиксели и засветы', impl:'интерактивно', kind:'display', interactive:true },
  { id:'ext', tag:'EXT', name:'Внешний монитор', method:'Подключённые мониторы и тип выхода (HDMI / DisplayPort / VGA)', impl:'реальные данные', kind:'runner', fetch:'ext' },
  { id:'bright', tag:'BRT', name:'Яркость', method:'Регулировка подсветки матрицы через WMI, проверка на глаз', impl:'реальное управление', kind:'brightness', interactive:true },
  { id:'cam', tag:'CAM', name:'Камера', method:'Живое превью через getUserMedia — оценка на глаз', impl:'реальное превью', kind:'camera', interactive:true },
  { id:'pad', tag:'PAD', name:'Тачпад', method:'Точки касания, мультитач, базовые жесты', impl:'интерактивно', kind:'touchpad', interactive:true },
  { id:'fp', tag:'FP', name:'Отпечаток', method:'Сенсор виден системе (WinBio) — регистрация вручную', impl:'частично', kind:'runner', fetch:'fp' },
  { id:'bat', tag:'BAT', name:'Аккумулятор', method:'Design vs Full charge capacity, циклы, износ (powercfg)', impl:'реальные данные', kind:'runner', fetch:'bat' },
  { id:'headset', tag:'HDP', name:'Наушники', method:'Гнездо 3,5 мм: тон в наушники (левый/правый канал); не подключены — не применимо', impl:'интерактивно', kind:'headset', interactive:true },
  { id:'touch', tag:'TCH', name:'Сенсорный экран', method:'Точки касания и мультитач; экрана нет — не применимо', impl:'интерактивно', kind:'touch', interactive:true },
  { id:'snd', tag:'SND', name:'Звук', method:'Тестовый сигнал (Web Audio) и echo-тест через микрофон', impl:'реально', kind:'audio', interactive:true },
  { id:'diskread', group:'disk', sub:'Чтение', tag:'RD', name:'Диск: чтение', method:'Замер скорости чтения по всему диску, медленные блоки и ошибки чтения', impl:'реальная нагрузка', kind:'diskread' },
  { id:'surface', group:'disk', sub:'Поверхность', tag:'SURF', name:'Диск: поверхность', method:'Чтение диска блоками с замером времени каждого блока и графиком скорости в реальном времени (как Victoria)', impl:'реальная нагрузка', kind:'surface' },
  { id:'diskwrite', group:'disk', sub:'Запись', tag:'WR', name:'Диск: запись', method:'Запись и чтение проверочного файла на томе: скорость по участкам, медленные блоки, ошибки данных', impl:'реальная нагрузка', kind:'diskwrite' },
  { id:'mem', tag:'RAM', name:'Память', method:'Многопоточная запись и проверка паттернов в ОЗУ, счётчик ошибок', impl:'реальная нагрузка', kind:'memtest' },
  { id:'fans', tag:'FAN', name:'Вентиляторы', method:'Обороты вентиляторов и отклик на управление скоростью (как в SpeedFan) — через датчики LibreHardwareMonitor', impl:'реальные данные', kind:'fans' },
  { id:'sens', tag:'SNS', name:'Датчики', method:'Температуры через WMI ACPI — доступность зависит от платы', impl:'зависит от платы', kind:'sensors' },
  { id:'stress', tag:'STR', name:'Стресс-тест', method:'Реальная нагрузка CPU на всех ядрах на заданное время', impl:'CPU реально', kind:'stress' }
];
/* Группы категорий: в ручном режиме показываются одной карточкой с подвкладками,
   в автопрогоне и отчёте каждый тест остаётся отдельным шагом. */
var GROUPS = {
  disk:{ name:'Диск', tag:'DISK', method:'Здоровье, SMART, чтение, сканирование поверхности и запись — подвкладки', impl:'реальные данные и нагрузка' }
};
function groupTests(g){ return CATS.filter(function(c){ return c.group===g; }); }
function isInteractive(id){ var c = CATS.filter(function(x){ return x.id===id; })[0]; return !!(c && c.interactive); }
function groupStatus(g){
  var ts = groupTests(g), done = 0, fail = false, pass = 0;
  ts.forEach(function(c){ var st = S.results[c.id]; if (st && st!=='idle'){ done++; if (st==='fail') fail = true; if (st==='pass' || st==='na') pass++; } });
  return { done:done, total:ts.length, st: fail ? 'fail' : (done===ts.length && ts.length) ? 'pass' : 'idle' };
}

var FILLS = [
  { name:'белый', color:'#FFFFFF' }, { name:'серый', color:'#8F8F8F' }, { name:'чёрный', color:'#000000' },
  { name:'красный', color:'#FF0000' }, { name:'зелёный', color:'#00FF00' }, { name:'синий', color:'#0000FF' },
  { name:'жёлтый', color:'#FFFF00' }, { name:'голубой', color:'#00FFFF' }, { name:'пурпурный', color:'#FF00FF' },
  { name:'градиент горизонтальный', color:'#666', bg:'linear-gradient(90deg,#000,#fff)' },
  { name:'градиент вертикальный', color:'#666', bg:'linear-gradient(180deg,#000,#fff)' },
  { name:'цветные полосы', color:'#888', bg:'linear-gradient(90deg,#fff 0 14.28%,#ff0 0 28.57%,#0ff 0 42.85%,#0f0 0 57.14%,#f0f 0 71.42%,#f00 0 85.7%,#00f 0)' },
  { name:'шахматка', color:'#888', bg:'repeating-conic-gradient(#000 0 25%,#fff 0 50%) 0 0/48px 48px' },
  { name:'сетка', color:'#444', bg:'linear-gradient(#fff 1px,transparent 1px) 0 0/40px 40px,linear-gradient(90deg,#fff 1px,transparent 1px) 0 0/40px 40px,#000' },
  { name:'полосы 1 пиксель', color:'#888', bg:'repeating-linear-gradient(90deg,#000 0 1px,#fff 1px 2px)' }
];
function fillBg(f){ return f.bg || f.color; }
var KEYROWS = [
  ['Esc','F1','F2','F3','F4','F5','F6','F7','F8','F9','F10','F11','F12','PrtScr','Ins','Del'],
  ['`','1','2','3','4','5','6','7','8','9','0','-','=','Bksp'],
  ['Tab','Q','W','E','R','T','Y','U','I','O','P','[',']','\\'],
  ['Caps','A','S','D','F','G','H','J','K','L',';',"'",'Enter'],
  ['Shift','Z','X','C','V','B','N','M',',','.','/','Shift'],
  ['Ctrl','Win','Alt','Space','Alt','Ctrl','←','↑','↓','→']
];
var WIDE = { Bksp:2, Tab:1.5, Caps:1.8, Enter:2.2, Shift:2.4, Space:6, Del:1.2 };
var TONES = ['1 кГц синус','Левый / правый','Echo-тест микрофона'];
var CAMCHECKS = ['Превью идёт без артефактов','Цвета не уходят в зелень','Фокус и резкость в норме','Индикатор активности горит'];
var STATUS = {
  pass:{ label:'пройдено', cls:'pass' },
  fail:{ label:'ошибка', cls:'fail' },
  na:{ label:'не применимо', cls:'na' },
  idle:{ label:'не проверено', cls:'' }
};

// Каталог драйверов — та же публичная ссылка, что в echips-driver-assistant.
var MANIFEST_PUBLIC_URL = "https://disk.360.yandex.ru/d/79yQHBN93UDZGg";

/* «Замена платы»: с v0.38.0 снова включена — завод прислал новую AMIDEWINx64 (2020 г., Aptio V) в
   src-tauri/assets/smbios. Если на каком-то BIOS утилита снова скажет «System doesn't support» —
   check_supported() покажет причину; выключить функцию: FEATURE_MB = false. */
var FEATURE_MB = true;

/* Вход по PIN включён (с v0.50.0): список инженеров подписан администратором, работает без интернета
   (вшитый + файл на флешке + обновление из приватного репозитория), просрочка 7 суток — только админ.
   Автовход под Максимом для тестов: FEATURE_PIN = false. */
var FEATURE_PIN = true;
var FEATURE_PAWNIO_UNINSTALL = false; // кнопка «Удалить драйвер PawnIO» скрыта по просьбе пользователя (код сохранён)

var S = {
  batLive: { on:false, data:null, t:null },
  screen:'start', cat:'usb', results:{}, comments:{},
  keys:{}, fill:0, padDots:[], padCount:0, padMax:0, padMoves:0,
  running:false, runLines:[], runError:null,
  tone:null, toneCtx:null, toneOsc:null, toneAnalyser:null, toneMic:null, phase:0,
  camStream:null,
  device:null, deviceError:null,
  sensorPoll:null, sensorReading:null, sensorHistory:[], gpuHistory:[],
  st:{ cfg:{ cpu:true, fpu:true, cache:false, memory:false, disk:false, gpu:false, dur:600, threads:'all', memPct:50 },
       running:false, elapsed:0, last:null, res:null, err:null, events:[], gpuFps:null,
       hist:{ load:[], temp:[], gpuT:[], clock:[], clockMax:0, scores:{} } },
  stressMarker:null,
  fan:{ poll:null, log:[], res:null, running:false, abort:false, manual:{}, seen:{}, touched:false, refresh:null },
  hwm:{ status:null, snap:null, busy:false, msg:'', err:'', confirm:null },
  snapshot:false, exported:null, reportSummary:'', kbWinBlock:false, kbWinBlockErr:'',
  hw:null, verdict:null, runActions:[], act:{ confirm:null, busy:false, msg:'', err:'', keyOpen:false, key:'' }, actRaw:null, detail:{}, kstat:{}, repId:null, markErr:null, br:{ info:null, loading:false }, camClip:null,
  rm:{ drives:null, timer:null, running:null, log:[], err:null, size:64 },
  dr:{ disks:null, sel:0, mode:64, running:false, pct:0, mbps:0, res:null, err:null },
  sm:{ disks:null, sel:0, err:null, loading:false },
  sf:{ disks:null, sel:0, range:'all', running:false, pos:0, total:0, mbps:0, startPct:0, endPct:100, classes:[0,0,0,0,0,0,0], bad:[], cols:[], res:null, err:null, t0:0 },
  dw:{ vols:null, live:[], sel:0, mb:512, running:false, pct:0, phase:'', mbps:0, res:null, err:null },
  mem:{ size:1024, passes:1, running:false, pct:0, pass:1, pattern:'', errors:0, res:null, err:null, t0:0 },
  auto:{ on:false, ids:[], idx:-1, stopped:false, waiting:false, msg:'', cls:'' },
  drv:{ step:'idle' },
  mb:{ step:'reading', techId:'', techName:'', pin:'', pinErr:'', ticket:'', serial:'', uuid:'', formErr:'', before:null, writeError:null },
  /* Общий вход по PIN при запуске (см. CLAUDE.md, задача №2). Список техников —
     _config/techs.json (приватный echips-reports) в публичном репозитории (подтягивается lockInit, только
     хэши PIN, см. commands/techs.rs). phase: boot → pin → verifying → ok → unlocked
     (или error, если нет сети и нет кэша). */
  lock:{ phase:'boot', techs:null, err:'', techId:'', pin:'', shake:false },
  engineer:null,
  techadmin:{ id:'', name:'', pin:'', role:'tech', err:'', result:'', busy:false, msg:'' },
  adminLog:[], adminPanelOpen:false
};

/* Кэш дорогих запросов (WMI/PowerShell): один и тот же список дисков не запрашивается
   заново в каждом тесте автопрогона. */
var _icache = {};
function invokeCached(cmd, args, ttl){
  var k = cmd + JSON.stringify(args||{}), e = _icache[k];
  if (e && Date.now()-e.t < ttl) return e.p;   // в кэше и незавершённый запрос — параллельные вызовы не дублируются
  var p = invoke(cmd, args).then(function(v){ return v; }, function(err){ delete _icache[k]; throw err; });
  _icache[k] = { t:Date.now(), p:p };
  return p;
}
/* В автопрогоне тяжёлые запросы (SMART, журнал сбоев) заранее запускаются параллельно и читаются из кэша;
   вне автопрогона — всегда свежие данные */
function invokeAuto(cmd, args){ return S.auto && S.auto.on ? invokeCached(cmd, args, 180000) : invoke(cmd, args); }

function esc(s){ return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }

/* Хэш PIN — SHA-256(salt+":"+pin) через Web Crypto (доступен в WebView2,
   secure context). Используется и на входе (lockSubmit сравнивает с
   pin_hash из _config/techs.json (приватный echips-reports)), и в генераторе (techadminGenerate) — один
   и тот же код с обеих сторон, поэтому дублировать алгоритм в Rust не нужно. */
function sha256Hex(text){
  var bytes = new TextEncoder().encode(text);
  return crypto.subtle.digest('SHA-256', bytes).then(function(buf){
    var arr = new Uint8Array(buf), hex = '';
    for (var i=0;i<arr.length;i++) hex += arr[i].toString(16).padStart(2,'0');
    return hex;
  });
}
function randomHex(numBytes){
  var arr = new Uint8Array(numBytes);
  crypto.getRandomValues(arr);
  var hex = '';
  for (var i=0;i<arr.length;i++) hex += arr[i].toString(16).padStart(2,'0');
  return hex;
}
function cat(){ for (var i=0;i<CATS.length;i++) if (CATS[i].id===S.cat) return CATS[i]; return CATS[0]; }
/* Сохранение подробностей теста для отчёта: строки лога, автовердикт, ряд точек графика. */
function recordDetail(id, patch){
  var d = S.detail[id] || {};
  Object.keys(patch).forEach(function(k){ d[k] = patch[k]; });
  d.ts = new Date().toISOString();
  S.detail[id] = d;
}
function downsample(arr, n){
  if (arr.length <= n) return arr.slice();
  var out = [], k = arr.length / n;
  for (var i=0;i<n;i++){ var a=Math.floor(i*k), b=Math.max(a+1, Math.floor((i+1)*k)), sum=0; for (var j=a;j<b;j++) sum+=arr[j]; out.push(Math.round(sum/(b-a))); }
  return out;
}
function inProfile(id){ return (profile().tests||[]).indexOf(id) >= 0; }

function statusOf(id){ return S.results[id] || 'idle'; }
function counts(){
  var p=0,f=0,n=0;
  CATS.forEach(function(c){
    if(S.results[c.id]==='pass') p++; else if(S.results[c.id]==='fail') f++; else if(S.results[c.id]==='na') n++;
  });
  return { pass:p, fail:f, na:n, checked:p+f+n };
}
/* Заглушки SMBIOS, которые производитель не заменил на настоящее значение */
function isPlaceholder(v){ return /^(default string|to be filled by o\.e\.m\.?|system manufacturer|system product name|o\.e\.m\.?|oem|not specified|unknown|none|n\/a|default)$/i.test(String(v||'').trim()); }
function cleanSmbios(v){ v = String(v||'').trim(); return isPlaceholder(v) ? '' : v; }
function deviceLabel(){
  if (!S.device) return 'определяется…';
  return (cleanSmbios(S.device.manufacturer) + ' ' + cleanSmbios(S.device.model)).trim() || 'неизвестная модель';
}
/* Событие в журнал сервера (контроль: начало и конец диагностики); без связи молча пропускается. */
function srvEvent(kind, data){ if (FEATURE_PIN) invoke('srv_event', { kind:kind, data:data||{} }).catch(function(){}); }
function deviceSn(){ return S.device ? String(S.device.serial_number||'').trim() : ''; }
/* Идентификатор устройства для отчёта и папок: серийник, а если он заглушка BIOS («To be filled by O.E.M.») —
   серийник платы, потом начало UUID, потом «БезСН» + приёмка. Иначе все ноутбуки без серийника попадали в одну папку. */
function deviceKey(){
  function good(v){ v = String(v||'').trim(); return v && !isPlaceholder(v) && !/^(system serial number|not applicable|0+|—)$/i.test(v) ? v : ''; }
  var sn = good(deviceSn()); if (sn) return sn;
  var hw = S.hw || {};
  var bsn = good(hw.board_serial); if (bsn) return bsn;
  var u = String(hw.system_uuid||'').replace(/[^0-9a-f]/gi,'').toLowerCase();
  if (u.length>=12 && !/^(0+|f+)$/.test(u) && u!=='03000200040005000006000700080009') return 'UUID-'+u.slice(0,12);
  return 'БезСН'+(S.intake ? '-'+S.intake : '');
}
function isAdmin(){ return !!(S.engineer && S.engineer.role==='admin'); }

/* ---------- окно: свернуть/закрыть ---------- */
(function initWindowControls(){
  var win = getCurrentWindow();
  var minBtn = document.getElementById('win-minimize');
  var closeBtn = document.getElementById('win-close');
  var maxBtn = document.getElementById('win-maximize');
  if (minBtn) minBtn.addEventListener('click', function(){ win.minimize(); });
  if (closeBtn) closeBtn.addEventListener('click', function(){ win.close(); });
  function toggleMax(){
    win.isMaximized().then(function(m){ return m ? win.unmaximize() : win.maximize(); }).catch(function(){});
  }
  function toggleFs(){
    win.isFullscreen().then(function(f){ return win.setFullscreen(!f); }).catch(function(){});
  }
  if (maxBtn) maxBtn.addEventListener('click', toggleMax);
  var tb = document.querySelector('.titlebar');
  if (tb) tb.addEventListener('dblclick', function(e){ if (!e.target.closest('.winbtn,button,a,input,.step,.techbox,.theme-toggle,.queuebox')) toggleMax(); });
  document.addEventListener('keydown', function(e){
    if (e.key==='F11' && !document.getElementById('fill-overlay')){ e.preventDefault(); toggleFs(); }
  });
  var siteLink = document.getElementById('site-link');
  if (siteLink) siteLink.addEventListener('click', function(e){
    e.preventDefault();
    if (window.__TAURI__.shell) window.__TAURI__.shell.open('https://echips.ru');
  });
})();

/* ---------- загрузка данных устройства при старте ---------- */
/* Профили моделей с сервера (см. commands/profiles.rs): подмешиваются к ECHIPS_PROFILES —
   новая модель добавляется кнопкой админа, без пересборки exe. */
function applyRemoteProfiles(file){
  if (!file || !file.profiles) return;
  var P = window.ECHIPS_PROFILES = window.ECHIPS_PROFILES || { default:{ name:'Стандартный', tests:[], expect:{} }, models:{} };
  P.models = P.models || {};
  var m = file.profiles.models || {};
  Object.keys(m).forEach(function(k){ P.models[k] = m[k]; });
  if (file.profiles['default'] && typeof file.profiles['default']==='object'){
    Object.keys(file.profiles['default']).forEach(function(k){ P['default'][k] = file.profiles['default'][k]; });
  }
  S.profilesSource = file.source || '';
}
function loadProfiles(){
  invoke('fetch_profiles').then(function(f){ applyRemoteProfiles(f); render(); }).catch(function(){});
}
function loadDevice(){
  invoke('get_system_info').then(function(info){
    S.device = info;
    render();
    invoke('get_hardware_summary').then(function(hw){ S.hw = hw; render(); }).catch(function(){});
    A.hwmRefresh();
    invoke('get_stress_marker').then(function(m){ if (m){ S.stressMarker = m; render(); } }).catch(function(){});
  }).catch(function(err){
    S.deviceError = typeof err === 'string' ? err : 'Не удалось определить устройство';
    render();
  });
}

/* ---------- действия ---------- */
/* Реальные серийники бывают длинными (напр. BM156ULRH003110125121000097 — 26 символов); лимит 8–20 не давал записать. */
function isValidSerial(v){ return /^[A-Za-z0-9][A-Za-z0-9._-]{3,39}$/.test(v); }
function isValidUuid(v){ return /^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$/.test(v); }

function stopSensorPoll(){
  if (S.sensorPoll){ clearInterval(S.sensorPoll); S.sensorPoll = null; }
}
function stopCamera(){
  if (S.camStream){ S.camStream.getTracks().forEach(function(t){ t.stop(); }); S.camStream = null; }
}
function stopAudio(){
  if (S.toneOsc){ try{ S.toneOsc.stop(); }catch(e){} S.toneOsc=null; }
  if (S.toneMic){ S.toneMic.getTracks().forEach(function(t){ t.stop(); }); S.toneMic=null; }
  if (S.toneCtx){ try{ S.toneCtx.close(); }catch(e){} S.toneCtx=null; }
  S.toneAnalyser = null;
}

var A = {
  go:function(screen,id){
    if(screen==='techadmin' && !isAdmin()) screen='start'; // экран только для администратора (см. isAdmin)
    if(screen==='mb' && !FEATURE_MB) screen='start';
    if((screen==='history' || screen==='events') && !isAdmin()) screen='start';   // история отчётов и журнал — только админ
    stopSensorPoll(); stopCamera(); stopAudio();
    if (S.rm.timer){ clearInterval(S.rm.timer); S.rm.timer=null; }
    if (S.kbT){ clearInterval(S.kbT); S.kbT=null; }
    if (S.fan.poll){ clearInterval(S.fan.poll); S.fan.poll=null; }
    if (S.fan.refresh){ clearInterval(S.fan.refresh); S.fan.refresh=null; }
    S.fan.abort = true; S.fan.manual = {};
    if (S.fan.touched){ invoke('hwmon_fan_default_all').catch(function(){}); S.fan.touched = false; }
    S.br = { info:null, loading:false }; S.camClip = null;
    if (S.st.running && screen!=='stress') invoke('stop_stress').catch(function(){});
    if (S.sf.running) invoke('stop_surface_scan').catch(function(){});
    if (S.dw.running) invoke('stop_disk_write_test').catch(function(){});
    if (S.dr.running) invoke('stop_disk_read_test').catch(function(){});
    if (S.mem.running) invoke('stop_memory_test').catch(function(){});
    if (S.kbWinBlock){ invoke('stop_win_key_block').catch(function(){}); S.kbWinBlock = false; }
    if (document.getElementById('fill-overlay')) A.fillClose();
    if (S.auto.on && screen!=='test' && screen!=='report' && screen!=='sensors' && screen!=='stress') A.autoOff();
    S.screen=screen; if(id) S.cat=id; S.running=false; S.runLines=[]; S.runError=null; S.verdict=null; S.runActions=[]; S.act={ confirm:null, busy:false, msg:'', err:'', keyOpen:false, key:'', autoFixDone:false }; S.exported=null; S.tone=null;
    if(screen==='techadmin'){ A.techadminInit(); }
    if(screen==='drivers'){ A.drvStart(); }
    if(screen==='mb'){ A.mbReset(); }
    if(screen==='sensors'){ A.sensorsStart(); }
    if(screen==='history'){ A.histLoad(); }
    if(screen==='events'){ A.evLoad(); }
    render();
  },
  openCat:function(id){
    var c=null; CATS.forEach(function(x){ if(x.id===id) c=x; });
    stopCamera(); stopAudio();
    A.go(c.kind==='sensors'?'sensors':c.kind==='stress'?'stress':'test', id);
    if (c.kind==='camera') A.camStart();
    if (c.kind==='headset' || c.kind==='touch') A.inputProbe(c);
    if (c.kind==='touchpad') A.padProbe(c);
    if (S.auto.on){
      if (c.kind==='runner') A.run();
      else if (c.kind==='diskread') A.drAuto();
      else if (c.kind==='diskwrite') A.dwAuto();
      else if (c.kind==='smart') A.smAuto();
      else if (c.kind==='surface') A.sfAuto();
      else if (c.kind==='memtest') A.memAuto();
      else if (c.kind==='sensors') A.sensorsAuto();
      else if (c.kind==='stress') A.stressAuto();
      else if (c.kind==='fans') A.fanTest(false);
    }
  },
  reset:function(){ _icache = {}; S.detail = {}; A.autoOff(); S.rm.log=[]; S.results={}; S.comments={}; S.keys={}; S.snapshot=false; S.reportSummary=''; S.startedAt = new Date().toISOString(); S.sentHash = null; render(); },
  press:function(id){ S.keys[id]=true; render(); },
  kbReset:function(){ S.keys={}; S.kstat={}; S.lastUnknown=null; render(); },
  nextFill:function(){ S.fill=(S.fill+1)%FILLS.length; render(); paintFill(); },
  prevFill:function(){ S.fill=(S.fill+FILLS.length-1)%FILLS.length; render(); paintFill(); },
  fillOpen:function(){
    if (document.getElementById('fill-overlay')) return;
    var o = document.createElement('div');
    o.id = 'fill-overlay';
    o.innerHTML = '<span id="fill-hint"></span>';
    o.addEventListener('click', function(){ A.nextFill(); });
    document.body.appendChild(o);
    try {
      var w = getCurrentWindow();
      // Из развёрнутого окна (maximize) fullscreen на безрамочном окне оставляет снизу полосу
      // на месте панели задач — поэтому сначала снимаем maximize, потом включаем fullscreen.
      w.isFullscreen().then(function(f){
        S.fsBefore = f; if (f) return null;
        return w.isMaximized().then(function(mx){
          S.maxBefore = mx;
          return (mx ? w.unmaximize() : null);
        }).then(function(){ return w.setFullscreen(true); });
      }).then(function(){ setTimeout(function(){ window.dispatchEvent(new Event('resize')); }, 150); }).catch(function(){});
    } catch(e){}
    paintFill();
    clearTimeout(S.fillHintT);
    S.fillHintT = setTimeout(function(){ var h=document.getElementById('fill-hint'); if(h) h.style.opacity='0'; }, 3500);
  },
  fillClose:function(){
    var o = document.getElementById('fill-overlay');
    if (o) o.parentNode.removeChild(o);
    try {
      if (!S.fsBefore){
        var w = getCurrentWindow();
        w.setFullscreen(false).then(function(){ if (S.maxBefore) return w.maximize(); }).catch(function(){});
      }
    } catch(e){}
    render();
  },
  setFill:function(i){ S.fill=i; render(); },
  comment:function(v){ S.comments[S.cat]=v; if (S.markErr && S.markErr.id===S.cat){ S.markErr=null; var m=document.getElementById('mark-err'); if(m) m.style.display='none'; } },
  reportSummary:function(v){ S.reportSummary=v; },
  /* Блокировка клавиши Win на время теста клавиатуры — только вручную по
     кнопке, не сама по себе при входе в тест (см. keyhook.rs). */
  kbWinToggle:function(){
    S.kbWinBlockErr = '';
    if (S.kbWinBlock){
      invoke('stop_win_key_block').catch(function(){});
      S.kbWinBlock = false; render(); return;
    }
    invoke('start_win_key_block').then(function(){
      S.kbWinBlock = true; render();
    }).catch(function(err){
      S.kbWinBlockErr = typeof err==='string' ? err : 'Не удалось включить блокировку Win'; render();
    });
  },
  mark:function(v){
    var id = S.cat, d = S.detail[id] || {}, auto = d.auto;
    var busy = (id==='diskread' && S.dr.running) || (id==='surface' && S.sf.running) || (id==='diskwrite' && S.dw.running) ||
      (id==='mem' && S.mem.running) || (id==='stress' && S.st.running) || (id==='fans' && S.fan.running) ||
      (cat().kind==='runner' && S.running);
    if (busy){
      S.markErr = { id:id, text:'Тест ещё выполняется — дождитесь результата или остановите его кнопкой «Остановить», затем отметьте вердикт. Иначе он прервётся и запишется без данных.' };
      render(); return;
    }
    // автовердикт можно изменить, но только с объяснением в комментарии — иначе
    // отчёт получается противоречивым («пройден» рядом с «SMART — тревога»)
    if (auto && auto.status && auto.status!==v){
      var cm = (S.comments[id]||'').trim();
      if (!cm || cm===auto.note){
        S.markErr = { id:id, text:'Автооценка: '+({pass:'пройден',fail:'не пройден',na:'не применимо'}[auto.status]||auto.status)+' — '+auto.note+'. Чтобы выбрать другой результат, допишите в комментарии причину.' };
        render(); return;
      }
      recordDetail(id, { override:{ from:auto.status, to:v, reason:cm } });
    } else if (d.override){ recordDetail(id, { override:null }); }
    // «Пройден» для клавиатуры ставится и без нажатия всех клавиш (нет цифрового блока, клавиша не нужна и т. п.):
    // какие клавиши нажаты, а какие нет, остаётся в подробностях отчёта (kbSummaryLines)
    if (cat().kind==='runner' && S.runError && v==='pass' && !(S.comments[id]||'').trim()){
      // проверка упала с ошибкой (нет данных) — «пройден» без объяснения давал в отчёте пустую строку «pass»
      S.markErr = { id:id, text:'Проверка не выполнилась ('+S.runError+'). Чтобы засчитать «пройден» без данных, допишите в комментарии, почему.' };
      render(); return;
    }
    S.markErr = null;
    if (cat().kind==='keyboard') recordDetail(id, { lines: kbSummaryLines() });
    recordDetail(id, { final:v });
    S.results[id]=v;
    if (S.auto.on) A.autoAfter(v); else A.go('dash');
  },
  snapshot:function(){ S.snapshot=true; render(); },
  exp:function(t){ A.exportReport(t); },
  repOpen:function(id){ S.repId = id; A.go('repdetail'); },

  /* ---- автопрогон по профилю модели ---- */
  /* Наушники / сенсорный экран: устройства определяет get_input_devices; в автопрогоне нет устройства → «не применимо». */
  inputProbe:function(c){
    S.tch = { dots:[], count:0, max:0, moves:0, active:{}, touched:false };
    S.inp = null;
    invoke('get_input_devices').then(function(d){
      S.inp = d; render();
      if (S.auto.on && S.cat===c.id){
        if (c.kind==='headset' && !d.headset) A.autoApply({ status:'na', note:'Наушники не подключены к гнезду — не применимо' });
        if (c.kind==='touch' && !d.touch) A.autoApply({ status:'na', note:'Сенсорный экран не обнаружен — не применимо' });
      }
    }).catch(function(){
      S.inp = { touch:false, headset:false, headset_name:'', error:true }; render();
      if (S.auto.on && S.cat===c.id) A.autoApply({ status:'na', note:'Не удалось определить устройство ('+(c.kind==='touch'?'сенсорный экран':'наушники')+') — не применимо' });
    });
  },
  autoStart:function(mode){
    // mode: 'express' — короткий набор проверок для входного контроля (P.expressTests), иначе полный
    S.autoMode = mode==='express' ? 'express' : 'full';
    var listSrc = S.autoMode==='express' ? (profile().expressTests || profile().tests) : profile().tests;
    var ids = (listSrc||[]).filter(function(id){ return CATS.some(function(c){ return c.id===id; }); });
    if (!ids.length) return;
    // Сначала тесты, требующие участия инженера (клавиатура, экран, тачпад,
    // яркость, камера, звук, флешка), затем полностью автоматические —
    // порядок внутри каждой группы как в профиле, состав не меняется.
    ids = ids.filter(function(id){ return isInteractive(id); }).concat(ids.filter(function(id){ return !isInteractive(id); }));
    S.results={}; S.comments={}; S.keys={}; S.snapshot=false; S.reportSummary='';
    S.startedAt = new Date().toISOString(); S.sentHash = null;
    S.auto = { on:true, ids:ids, idx:-1, stopped:false, waiting:false, msg:'', cls:'', timer:null, mode:S.autoMode };
    srvEvent('auto_start', { mode:S.autoMode||'full', tests:ids.length, model:deviceLabel(), serial:deviceKey(), intake:S.intake||'', stage:S.repairStage||'' });
    S.autoTemps = []; S.autoLog = [];
    A.autoTempPoll(true);
    _icache = {};
    // тяжёлые запросы к диску и журналу собираем заранее, параллельно (тесты возьмут готовое из кэша)
    if (ids.indexOf('smart')>=0) invokeCached('get_smart_report', {}, 180000).catch(function(){});
    if (ids.indexOf('disk')>=0) invokeCached('get_disk_health', {}, 180000).catch(function(){});
    if (ids.indexOf('crash')>=0) invokeCached('get_crash_history', { days: Math.max(profile().crashDays || 30, 90) }, 180000).catch(function(){});
    A.autoNext();
  },
  autoOff:function(){
    A.autoTempPoll(false);
    if (S.auto.timer) clearTimeout(S.auto.timer);
    if (S.auto.probe) clearTimeout(S.auto.probe);
    if (S.st.running) invoke('stop_stress').catch(function(){});
    S.auto = { on:false, ids:[], idx:-1, stopped:false, waiting:false, msg:'', cls:'' };
  },
  /* Живая температура процессора на время автопрогона (для экрана «Идёт проверка»): опрос раз в 3 с, без полной перерисовки */
  autoTempPoll:function(on){
    if (S.autoTempT){ clearInterval(S.autoTempT); S.autoTempT = null; }
    if (!on) return;
    function tick(){
      invoke('get_thermal_reading').then(function(r){
        if (r && r.available && r.cpu_temp_c!=null){ S.autoTemps.push(r.cpu_temp_c); if (S.autoTemps.length>60) S.autoTemps.shift(); autoPaintTemp(); }
      }).catch(function(){});
    }
    tick(); S.autoTempT = setInterval(tick, 3000);
  },
  autoDetail:function(){ S.auto.detail = !S.auto.detail; render(); },
  autoStop:function(){ srvEvent('auto_abort', { at:S.auto.idx+1, of:S.auto.ids.length, test:(S.auto.ids[S.auto.idx]||'') }); A.autoOff(); A.go('dash'); },
  autoReport:function(){
    var wasAuto = S.auto.on;
    if (wasAuto){ var cc = counts(); srvEvent('auto_end', { pass:cc.pass, fail:cc.fail, model:deviceLabel(), serial:deviceKey() }); }
    A.autoOff();
    if (wasAuto) A.reportSync('auto');
    A.go('report');
  },
  autoNext:function(){
    var a = S.auto; if (!a.on) return;
    if (a.timer) clearTimeout(a.timer);
    a.idx++; a.stopped=false; a.waiting=false; a.msg=''; a.cls=''; a.stepT0=Date.now();   // a.detail держится на весь прогон
    if (a.idx >= a.ids.length){ A.autoReport(); return; }
    var nm = (CATS.filter(function(x){ return x.id===a.ids[a.idx]; })[0]||{}).name || a.ids[a.idx];
    autoLogPush(nm+' — запуск');
    A.openCat(a.ids[a.idx]);
  },
  /* Итог шага: пройден/не применимо — идём дальше сами, ошибка — ждём техника. */
  autoAfter:function(status){
    var a = S.auto; if (!a.on) return;
    if (status==='fail' && profile().stopAtFail){ a.stopped=true; a.msg='Тест не пройден — автопрогон остановлен (StopAtFail).'; a.cls='err'; render(); return; }
    A.autoNext();
  },
  autoApply:function(v){
    var a = S.auto; if (!a.on) return;
    if (!v || !v.status){ a.waiting=true; a.msg='Автооценка невозможна — отметьте результат вручную.'; a.cls=''; render(); return; }
    S.results[S.cat]=v.status; S.comments[S.cat]=v.note; renderNav();
    recordDetail(S.cat, { auto: { status:v.status, note:v.note } });
    autoLogPush(cat().name+' — '+(v.status==='fail'?'не пройден':v.status==='na'?'не применимо':'пройден')+': '+v.note);
    var at = a.idx;
    if (v.status==='fail'){
      if (profile().stopAtFail){ a.stopped=true; a.msg='Не пройден: '+v.note+' — автопрогон остановлен.'; a.cls='err'; }
      else { a.waiting=true; a.msg='Не пройден: '+v.note; a.cls='err'; }
      render(); return;
    }
    a.msg=(v.status==='na'?'Не применимо: ':'Пройден: ')+v.note+' · переход к следующему…'; a.cls='ok'; render();
    a.timer = setTimeout(function(){ if (S.auto.on && S.auto.idx===at) A.autoNext(); }, 1800);
  },

  /* ---- активация Windows: устранение (каждый шаг — с подтверждением) ---- */
  actAsk:function(step){ S.act.confirm = step; S.act.msg=''; S.act.err=''; render(); },
  actCancel:function(){ S.act.confirm = null; render(); },
  actKeyToggle:function(){ S.act.keyOpen = !S.act.keyOpen; render(); },
  actKey:function(v){ S.act.key = v; },
  actOpen:function(){ invoke('open_activation_settings').catch(function(){}); },
  actRun:function(){
    var step = S.act.confirm; if (!step || S.act.busy) return;
    S.act.busy = true; S.act.err=''; render();
    invoke('run_activation_step', { step:step, key: step==='install_key' ? S.act.key : null }).then(function(r){
      S.act.busy = false; S.act.confirm = null; S.act.msg = r; S.act.key = '';
      A.run();   // повторная проверка статуса; в автопрогоне при успехе шаг завершится сам
    }).catch(function(err){
      var t = typeof err==='string' ? err : 'Не удалось выполнить шаг';
      var m = t.match(/0x[0-9A-Fa-f]{8}/);
      S.act.busy = false; S.act.confirm = null;
      S.act.err = t + (m && ACT_ERRORS[m[0].toUpperCase().replace('0X','0X')] ? ' — '+ACT_ERRORS[m[0].toUpperCase()] : '');
      render();
    });
  },

  /* ---- SMART ---- */
  smLoad:function(){
    if (S.sm.disks || S.sm.loading) return;
    S.sm.loading = true;
    invokeAuto('get_smart_report').then(function(list){
      S.sm.disks = list; S.sm.loading = false; S.sm.sel = 0; list.forEach(function(d,k){ if (d.is_system) S.sm.sel = k; }); render();
    }).catch(function(err){
      S.sm.loading = false; S.sm.disks = []; S.sm.err = typeof err==='string'?err:'Не удалось получить SMART'; render();
    });
  },
  smPick:function(i){ S.sm.sel = i; render(); },
  smAuto:function(){
    S.sm.disks = null; S.sm.err = null; S.sm.loading = true;
    invokeAuto('get_smart_report').then(function(list){
      // В автопрогоне — только системный диск; остальные проверяются вручную (вкладка «Категории»).
      var sysOnly = list.filter(function(d){ return d.is_system; });
      if (sysOnly.length) list = sysOnly;
      S.sm.disks = list; S.sm.loading = false; S.sm.sel = 0; list.forEach(function(d,k){ if (d.is_system) S.sm.sel = k; }); render();
      recordDetail('smart', { lines: smartLines(list) });
      A.autoApply(judgeSmart(list));
    }).catch(function(err){
      S.sm.loading = false; S.sm.disks = []; S.sm.err = typeof err==='string'?err:'Не удалось получить SMART'; render(); A.autoApply(null);
    });
  },

  /* ---- сканирование поверхности ---- */
  sfLoad:function(){
    if (S.sf.disks) return;
    S.sf.disks = [];
    invokeCached('get_disk_health', {}, 30000).then(function(list){
      S.sf.disks = list; var i = 0; list.forEach(function(d,k){ if (d.is_system) i = k; });
      S.sf.sel = i; render();
    }).catch(function(err){ S.sf.err = typeof err==='string'?err:'Не удалось получить список дисков'; render(); });
  },
  sfPick:function(i){ if(!S.sf.running){ S.sf.sel=i; render(); } },
  sfRange:function(r){ if(!S.sf.running){ S.sf.range=r; render(); } },
  sfStart:function(){
    var d = S.sf.disks && S.sf.disks[S.sf.sel]; if (!d || S.sf.running) return;
    var f = S.sf, gb = d.size_gb, r = f.range, start = 0, end = 100;
    if (r==='first100'){ end = Math.min(100, 100/gb*100); }
    else if (r==='first10'){ end = Math.min(100, 10/gb*100); }
    else if (r==='last10'){ start = Math.max(0, 100-10/gb*100); }
    else if (typeof r==='number'){ end = Math.min(100, r/gb*100); }
    f.startPct=start; f.endPct=end; f.running=true; f.pos=0; f.total=0; f.mbps=0; f.classes=[0,0,0,0,0,0,0]; f.bad=[]; f.cols=[]; f.res=null; f.err=null; f.t0=Date.now(); f.liveMin=null; f.liveMax=null;
    render();
    var unlisten=null;
    tauriEvent.listen('surface-progress', function(ev){
      var p=ev.payload; f.pos=p.pos_mb; f.total=p.total_mb; f.mbps=p.mbps; f.classes=p.classes; f.bad=f.bad.concat(p.new_bad_mb);
      if (p.mbps>0){ f.liveMin = f.liveMin==null ? p.mbps : Math.min(f.liveMin,p.mbps); f.liveMax = f.liveMax==null ? p.mbps : Math.max(f.liveMax,p.mbps); }
      // Колонка графика — по доле пройденного диапазона (pos_mb/total_mb уже
      // относительны началу скана), а не по абсолютной позиции на диске —
      // иначе при сканировании части диска (не «весь диск») график почти
      // весь оставался пустым: начало/конец полосы не совпадали с
      // началом/концом самого теста (замечание техника по реальному отчёту).
      var COLS=SF_COLS, frac=p.total_mb>0 ? p.pos_mb/p.total_mb : 0;
      var col=Math.min(COLS-1, Math.max(0, Math.floor(frac*COLS)));
      if (p.mbps>0){ var lastc=f.lastCol==null ? col : f.lastCol; for (var c=Math.min(lastc,col); c<=col; c++) f.cols[c]=p.mbps; }
      f.lastCol=col; paintSurface();
    }).then(function(u){ unlisten=u; });
    function fin(){ if(unlisten) unlisten(); f.running=false; f.lastCol=null; }
    invoke('run_surface_scan', { diskNumber:d.number, sizeGb:d.size_gb, startPct:start, endPct:end, blockKb:512 }).then(function(res){
      fin(); f.res=res; render(); paintSurface();
      recordDetail('surface', { lines:['Просканировано '+(res.scanned_mb/1024).toFixed(1)+' ГБ за '+res.elapsed_secs+' с: скорость средняя '+res.avg_mbps.toFixed(0)+', мин '+res.min_mbps.toFixed(0)+', макс '+res.max_mbps.toFixed(0)+' МБ/с', 'Задержки блоков: <5 мс '+res.classes[0]+' · <20 '+res.classes[1]+' · <50 '+res.classes[2]+' · <150 '+res.classes[3]+' · <500 '+res.classes[4]+' · ≥500 '+res.classes[5]+' · ошибок '+res.classes[6]].concat(res.bad_offsets_mb.length?['Нечитаемые блоки (МБ): '+res.bad_offsets_mb.slice(0,50).join(', ')]:[]), series: downsample(f.cols.filter(function(v){ return v!=null; }), 200) });
      if (S.auto.on && S.cat==='surface'){
        var total = res.classes.reduce(function(a,b){ return a+b; }, 0) || 1;
        var slowPct = res.classes[4]/total*100, maxSlow = profile().surfaceSlowPct!=null ? profile().surfaceSlowPct : 1;
        A.autoApply(res.stopped ? null
          : res.classes[6]>0 ? { status:'fail', note:'Нечитаемых блоков: '+res.classes[6]+' (первые смещения, МБ: '+res.bad_offsets_mb.slice(0,5).join(', ')+')' }
          : res.classes[5]>0 ? { status:'fail', note:'Блоков с задержкой ≥500 мс: '+res.classes[5]+' — деградация поверхности' }
          : slowPct>maxSlow ? { status:'fail', note:'Медленных блоков (150–500 мс): '+slowPct.toFixed(1)+'% (порог '+maxSlow+'%)' }
          : { status:'pass', note:'Прочитано '+(res.scanned_mb/1024).toFixed(1)+' ГБ без ошибок, '+res.avg_mbps.toFixed(0)+' МБ/с в среднем' });
      }
    }).catch(function(err){
      fin(); f.err = typeof err==='string'?err:'Ошибка сканирования'; render();
      if (S.auto.on && S.cat==='surface') A.autoApply(null);
    });
  },
  sfStop:function(){ invoke('stop_surface_scan').catch(function(){}); },
  sfAuto:function(){
    var f=S.sf; f.res=null; f.err=null; f.running=false; f.disks=null;
    invokeCached('get_disk_health', {}, 30000).then(function(list){
      if (!list.length) throw 'Физические диски не найдены';
      f.disks=list; f.sel=0; list.forEach(function(d,k){ if (d.is_system) f.sel=k; });
      f.range = profile().surfaceScanGb || 20; A.sfStart();
    }).catch(function(err){ f.err = typeof err==='string'?err:'Не удалось получить список дисков'; render(); A.autoApply(null); });
  },

  /* ---- автозапуск длинных тестов в автопрогоне ---- */
  drAuto:function(){
    S.dr.res=null; S.dr.err=null; S.dr.running=false; S.dr.disks=null;
    invokeCached('get_disk_health', {}, 30000).then(function(list){
      if (!list.length) throw 'Физические диски не найдены';
      S.dr.disks = list; S.dr.sel = 0; list.forEach(function(d,k){ if (d.is_system) S.dr.sel = k; });
      S.dr.mode = profile().diskReadSampleMb || 256; A.drStart();   // расширенный: 24 × 256 МБ = 6 ГБ
    }).catch(function(err){ S.dr.err = typeof err==='string'?err:'Не удалось получить список дисков'; render(); A.autoApply(null); });
  },
  dwAuto:function(){
    S.dw.res=null; S.dw.err=null; S.dw.running=false; S.dw.vols=null;
    invokeCached('list_fixed_volumes', {}, 30000).then(function(list){
      if (!list.length) throw 'Тома с буквами не найдены';
      S.dw.vols = list; S.dw.sel = 0; list.forEach(function(v,k){ if (v.is_system) S.dw.sel = k; });
      // 10 ГБ (профиль), но не больше, чем позволяет свободное место тома (запас как в write_test: 512 МБ или 5%)
      var vw = list[S.dw.sel], want = profile().diskWriteMb || 512, freeMb = vw && vw.free_gb!=null ? vw.free_gb*1024 : null;
      if (freeMb!=null){
        var room = freeMb - Math.max(512, freeMb/20) - 256;
        [want, 6144, 2048, 512, 256].some(function(x){ if (x<=want && x<=room){ want = x; return true; } return false; });
        if (want>room) want = Math.max(128, Math.floor(room));
      }
      S.dw.mb = want; A.dwStart();
    }).catch(function(err){ S.dw.err = typeof err==='string'?err:'Не удалось получить список томов'; render(); A.autoApply(null); });
  },
  memAuto:function(){
    S.mem.size = 0; S.mem.passes = S.autoMode==='express' ? 1 : (profile().memPasses || 2); S.mem.res=null; S.mem.err=null;
    // синие экраны в журнале (тест «Журнал сбоев» не пройден) — память проверяем дольше: 4 прохода вместо 2
    if (S.results.crash==='fail' && S.autoMode!=='express'){ S.mem.passes = Math.max(S.mem.passes, 4); autoLogPush('Журнал сбоев не пройден — память проверяется 4 прохода'); }
    A.memStart();
  },
  sensorsAuto:function(){
    S.sensorHistory=[]; S.gpuHistory=[];
    var secs = profile().sensorsProbeSecs || 8, at = S.auto.idx;
    S.auto.msg = 'Снимаем показания датчиков ('+secs+' с)…'; S.auto.cls=''; render();
    S.auto.probe = setTimeout(function(){
      if (!S.auto.on || S.auto.idx!==at) return;
      var temps = S.sensorHistory.concat(S.gpuHistory), max = profile().maxTempC || 95;
      recordDetail('sens', { lines: temps.length ? ['Замеров: '+temps.length+', максимум '+Math.max.apply(null,temps).toFixed(0)+' °C, минимум '+Math.min.apply(null,temps).toFixed(0)+' °C'] : ['Температурные датчики недоступны'] });
      if (!temps.length) A.autoApply({ status:'na', note:'Температурные датчики недоступны (ACPI/nvidia-smi) — см. LibreHardwareMonitor' });
      else {
        var t = Math.max.apply(null, temps);
        A.autoApply(t>=max ? { status:'fail', note:'Температура '+t.toFixed(0)+' °C в простое не ниже порога '+max+' °C' }
                           : { status:'pass', note:'Датчики отвечают, максимум '+t.toFixed(0)+' °C'+(temps.length>=3 && Math.min.apply(null,temps)===t ? ' (значение не меняется — вероятно, фиксированный ACPI-датчик, реальную температуру не отражает)' : '') });
      }
    }, secs*1000);
  },
  stressAuto:function(){
    if (S.st.running) return;
    var P = profile(), list = P.stressStressors || ['cpu','fpu'];
    var c = S.st.cfg;
    c.cpu = list.indexOf('cpu')>=0; c.fpu = list.indexOf('fpu')>=0; c.cache = list.indexOf('cache')>=0;
    c.memory = list.indexOf('memory')>=0; c.disk = list.indexOf('disk')>=0; c.gpu = list.indexOf('gpu')>=0;
    c.dur = P.stressSecs || 60; c.threads = 'all'; c.memPct = 50;
    S.auto.msg = 'Стресс-тест: '+list.join(' + ')+', '+c.dur+' с…'; S.auto.cls=''; render();
    A.stStart();
  },

  /* ---- тест чтения диска ---- */
  drLoad:function(){
    if (S.dr.disks) return;
    S.dr.disks = [];
    invokeCached('get_disk_health', {}, 30000).then(function(list){
      S.dr.disks = list;
      var i = 0; list.forEach(function(d,k){ if (d.is_system) i = k; });
      S.dr.sel = i; render();
    }).catch(function(err){ S.dr.err = typeof err==='string'?err:'Не удалось получить список дисков'; render(); });
  },
  drPick:function(i){ if(!S.dr.running){ S.dr.sel=i; render(); } },
  drMode:function(m){ if(!S.dr.running){ S.dr.mode=m; render(); } },
  drStart:function(){
    var d = S.dr.disks[S.dr.sel]; if (!d || S.dr.running) return;
    S.dr.running=true; S.dr.pct=0; S.dr.mbps=0; S.dr.res=null; S.dr.err=null; render();
    var unlisten=null;
    tauriEvent.listen('disk-progress', function(ev){
      S.dr.pct=ev.payload.pct; S.dr.mbps=ev.payload.mbps;
      var f=document.getElementById('dr-fill'), t=document.getElementById('dr-txt');
      if (f && t){ f.style.width=S.dr.pct+'%'; t.textContent=S.dr.pct+'% · '+S.dr.mbps.toFixed(0)+' МБ/с'; } else render();
    }).then(function(u){ unlisten=u; });
    function fin(){ if(unlisten) unlisten(); S.dr.running=false; }
    invoke('run_disk_read_test', { diskNumber:d.number, sizeGb:d.size_gb, sampleMb:S.dr.mode }).then(function(r){
      fin(); S.dr.res=r; render();
      recordDetail('diskread', { lines:['Чтение 24 участков диска: средняя '+r.avg_mbps.toFixed(0)+' МБ/с, мин '+r.min_mbps.toFixed(0)+', макс '+r.max_mbps.toFixed(0), 'Медленных блоков: '+r.slow_blocks+', ошибок чтения: '+r.errors+', прочитано '+r.read_mb+' МБ'+(r.stopped?' (остановлено)':'')], series: r.samples.map(Math.round) });
      if (S.auto.on && S.cat==='diskread'){
        var slowMax = profile().diskSlowBlocksMax!=null ? profile().diskSlowBlocksMax : 3;
        A.autoApply(r.stopped ? null
          : r.errors>0 ? { status:'fail', note:'Ошибок чтения: '+r.errors+' (смещения, МБ: '+r.error_offsets_mb.slice(0,5).join(', ')+')' }
          : r.slow_blocks>slowMax ? { status:'fail', note:'Медленных блоков: '+r.slow_blocks+' (допустимо '+slowMax+') — возможна деградация диска' }
          : { status:'pass', note:'Чтение без ошибок: '+r.avg_mbps.toFixed(0)+' МБ/с в среднем, минимум '+r.min_mbps.toFixed(0)+', медленных блоков '+r.slow_blocks });
      }
    }).catch(function(err){
      fin(); S.dr.err = typeof err==='string'?err:'Ошибка теста чтения'; render();
      if (S.auto.on && S.cat==='diskread') A.autoApply(null);
    });
  },
  drStop:function(){ invoke('stop_disk_read_test').catch(function(){}); },

  /* ---- тест записи диска ---- */
  dwLoad:function(){
    if (S.dw.vols) return;
    S.dw.vols = [];
    invokeCached('list_fixed_volumes', {}, 30000).then(function(list){
      S.dw.vols = list; var i = 0; list.forEach(function(v,k){ if (v.is_system) i = k; });
      S.dw.sel = i; render();
    }).catch(function(err){ S.dw.err = typeof err==='string'?err:'Не удалось получить список томов'; render(); });
  },
  dwPick:function(i){ if(!S.dw.running){ S.dw.sel=i; render(); } },
  dwMode:function(m){ if(!S.dw.running){ S.dw.mb=m; render(); } },
  dwStart:function(){
    var v = S.dw.vols && S.dw.vols[S.dw.sel]; if (!v || S.dw.running) return;
    var d = S.dw; d.running=true; d.pct=0; d.mbps=0; d.phase='write'; d.res=null; d.err=null; d.live=[]; render();
    var unlisten=null;
    tauriEvent.listen('diskw-progress', function(ev){
      d.pct=ev.payload.pct; d.mbps=ev.payload.mbps; d.phase=ev.payload.phase;
      if (d.phase==='write') d.live.push(ev.payload.mbps);
      var lv=document.getElementById('dw-live'); if (lv && d.phase==='write') lv.innerHTML=sparkInner(d.live);
      var f=document.getElementById('dw-fill'), t=document.getElementById('dw-txt');
      if (f && t){ f.style.width=d.pct+'%'; t.textContent=d.pct+'% · '+(d.phase==='write'?'запись':'чтение')+' · '+d.mbps.toFixed(0)+' МБ/с'; } else render();
    }).then(function(u){ unlisten=u; });
    function fin(){ if(unlisten) unlisten(); d.running=false; }
    var sizeMb = d.mb===0 ? Math.max(128, Math.floor(v.free_gb*1024*0.85)) : d.mb;
    invoke('run_disk_write_test', { letter:v.letter, sizeMb:sizeMb }).then(function(r){
      fin(); d.res=r; render();
      recordDetail('diskwrite', { lines:['Том '+r.letter+': файл '+r.size_mb+' МБ · запись средняя '+r.write_avg_mbps.toFixed(0)+' МБ/с (мин '+r.write_min_mbps.toFixed(0)+', макс '+r.write_max_mbps.toFixed(0)+') · чтение обратно '+r.read_mbps.toFixed(0)+' МБ/с','Медленных блоков: '+r.slow_blocks+', несовпадений данных: '+r.errors+(r.stopped?' (остановлено)':'')], series: downsample(r.write_samples, 200) });
      if (S.auto.on && S.cat==='diskwrite'){
        var slowMax = profile().diskSlowBlocksMax!=null ? profile().diskSlowBlocksMax : 3;
        A.autoApply(r.stopped ? null
          : r.errors>0 ? { status:'fail', note:'Несовпадений данных при записи/чтении: '+r.errors }
          : r.slow_blocks>slowMax ? { status:'fail', note:'Медленных блоков: '+r.slow_blocks+' (допустимо '+slowMax+') — возможна деградация диска' }
          : { status:'pass', note:'Запись '+r.write_avg_mbps.toFixed(0)+' МБ/с (мин '+r.write_min_mbps.toFixed(0)+'), чтение '+r.read_mbps.toFixed(0)+' МБ/с, данные совпали, том '+r.letter+':' });
      }
    }).catch(function(err){
      fin(); d.err = typeof err==='string'?err:'Ошибка теста записи'; render();
      if (S.auto.on && S.cat==='diskwrite') A.autoApply(null);
    });
  },
  dwStop:function(){ invoke('stop_disk_write_test').catch(function(){}); },

  /* ---- тест памяти ---- */
  /* Номер приёмки (левое меню): только цифры, до 6. Меняется путь отчёта → новая отправка. */
  /* Этап ремонта: «До ремонта» / «После ремонта» (повторный клик снимает выбор) — пишется в отчёт и в имя файла,
     вкладка «История» по нему сама находит пару для сравнения. */
  sendNow:function(){
    var b = document.getElementById('queue-send'); if (b) b.textContent = '…';
    invoke('flush_report_queue').catch(function(){}).then(function(){ S.sentHash = null; A.reportSync('sync'); setTimeout(function(){ refreshQueue(); if (b) b.textContent = 'отправить'; }, 2500); });
  },
  setStage:function(v){
    var next = S.repairStage===v ? '' : v;
    // Смена этапа после прогона раньше пересылала ТЕ ЖЕ результаты под другим этапом (пара «до/после» из одного прогона).
    // Теперь смена этапа при уже собранных результатах начинает новую проверку (отчёт предыдущей уже отправлен).
    var have = Object.keys(S.results).some(function(k){ return S.results[k] && S.results[k]!=='idle'; });
    if (have){
      var ok = true;
      try { ok = window.confirm('Сменить этап ремонта?\n\nТекущие результаты будут сброшены — начнётся новая проверка. Отчёт по этой проверке уже отправлен (если очередь пуста), к нему вернуться можно будет через «Историю».'); } catch(e){}
      if (!ok){ renderStageBtns(); return; }
      A.reportSync('manual');
      S.repairStage = next; A.reset(); return;
    }
    S.repairStage = next; renderStageBtns();
  },
  setIntake:function(v){ S.intake = String(v||'').replace(/\D/g,'').slice(0,6); var el=document.getElementById('intake-input'); if(el && el.value!==S.intake) el.value=S.intake; },
  profKey:function(v){ (S.profSave = S.profSave || {}).key = v; },
  profBios:function(v){ (S.profSave = S.profSave || {}).bios = !!v; },
  profMaxT:function(v){ (S.profSave = S.profSave || {}).maxT = v; },
  profSave:function(){
    var P = S.profSave; if (!P || P.busy || !S.hw) return;
    var key = (P.key||'').trim();
    if (!key){ P.err = 'Укажите ключ модели.'; P.msg=''; render(); return; }
    var e = profileFromHw(S.hw);
    P.busy = true; P.err = ''; P.msg = ''; render();
    // Сохраняем поверх существующего профиля модели: прочие поля (клавиатура, подсказки, пороги) не теряются
    var ex = ((window.ECHIPS_PROFILES||{}).models||{})[key] || {}, np = {}, ee = {};
    Object.keys(ex).forEach(function(k){ np[k] = ex[k]; });
    Object.keys(ex.expect||{}).forEach(function(k){ ee[k] = ex.expect[k]; });
    Object.keys(e).forEach(function(k){ ee[k] = e[k]; });
    if (P.bios && S.hw.bios_version){
      var list = (ee.biosAny || (ee.biosContains ? [ee.biosContains] : [])).slice();
      if (list.indexOf(S.hw.bios_version)<0) list.push(S.hw.bios_version);
      ee.biosAny = list; delete ee.biosContains;
    }
    np.name = np.name || key; np.expect = ee;
    var mt = parseFloat(P.maxT); if (mt>=60 && mt<=110) np.maxTempC = mt;
    invoke('profiles_save_model', { key:key, profile:np }).then(function(f){
      applyRemoteProfiles(f); P.msg = 'Эталон «'+key+'» сохранён и подписан — появится на других ноутбуках, когда они выйдут в интернет.';
    }).catch(function(err){ P.err = typeof err==='string' ? err : 'Не удалось сохранить эталон'; }).then(function(){ P.busy=false; render(); });
  },
  /* ---- история отчётов (админ): список из echips-reports, просмотр, сравнение двух ---- */
  histLoad:function(force){
    var H = S.hist = S.hist || { list:null, loading:false, err:'', q:'', sel:[], view:null, cmp:null, busy:false, sum:{}, sumBusy:false, fModel:'', fMode:'', fErr:false };
    if (H.loading || (H.list && !force)) return;
    H.loading = true; H.err = ''; render();
    invoke('list_reports').then(function(l){ H.list = l; }).catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось получить список отчётов'; H.list = []; })
      .then(function(){ H.loading = false; render(); });
  },
  histQ:function(v){ S.hist.q = v; render(); },
  histPick:function(idx){
    var H = S.hist, path = H.shown[idx].path, i = H.sel.indexOf(path);
    if (i>=0) H.sel.splice(i,1); else { H.sel.push(path); if (H.sel.length>2) H.sel.shift(); }
    render();
  },
  histOpen:function(idx){
    var H = S.hist, path = H.shown[idx].path; H.busy = true; H.err = ''; render();
    invoke('fetch_report', { path:path }).then(function(env){ H.view = { path:path, env:env }; H.cmp = null; })
      .catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось открыть отчёт'; }).then(function(){ H.busy = false; render(); });
  },
  histCompare:function(){
    var H = S.hist; if (H.sel.length!==2) return;
    H.busy = true; H.err = ''; render();
    // порядок: старый отчёт слева (по дате/имени файла), новый справа
    var paths = H.sel.slice().sort();
    Promise.all(paths.map(function(p){ return invoke('fetch_report', { path:p }); })).then(function(envs){
      H.cmp = { a:{ path:paths[0], env:envs[0] }, b:{ path:paths[1], env:envs[1] } }; H.view = null;
    }).catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось получить отчёты'; }).then(function(){ H.busy = false; render(); });
  },
  /* Одна кнопка: сравнить самый свежий отчёт «до ремонта» с самым свежим «после» (та же приёмка/ремонт) */
  histComparePair:function(){
    var H = S.hist, q = (H.q||'').toLowerCase().trim();
    var list = (H.list||[]).filter(function(x){ return !q || (x.device+' '+x.engineer+' '+x.date).toLowerCase().indexOf(q)>=0; });
    var pr = histFindPair(list); if (!pr) return;
    H.busy = true; H.err = ''; render();
    Promise.all([pr.before.path, pr.after.path].map(function(p){ return invoke('fetch_report', { path:p }); })).then(function(envs){
      H.cmp = { a:{ path:pr.before.path, env:envs[0] }, b:{ path:pr.after.path, env:envs[1] } }; H.view = null;
    }).catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось получить отчёты'; }).then(function(){ H.busy = false; render(); });
  },
  histFilter:function(k, v){ S.hist[k] = v; render(); },
  /* Сводки (модель, режим, пройдено/ошибок) читаются из самих отчётов — по 6 запросов параллельно, первые 60 строк списка */
  histLoadSums:function(){
    var H = S.hist; if (H.sumBusy) return;
    var q = (H.q||'').toLowerCase().trim(), list = histFiltered(H).slice(0,60).filter(function(x){ return !H.sum[x.path]; });
    if (!list.length) return;
    H.sumBusy = true; render();
    var i = 0;
    function next(){
      if (i>=list.length){ H.sumBusy = false; render(); return; }
      var batch = list.slice(i, i+6); i += 6;
      Promise.all(batch.map(function(x){
        return invoke('fetch_report', { path:x.path }).then(function(env){
          var r = env.report||{}, c = histSummary(r);
          H.sum[x.path] = { model:r.device_model||'', mode:r.run_mode||'', pass:c.pass, fail:c.fail, na:c.na };
        }).catch(function(){ H.sum[x.path] = { model:'', mode:'', pass:0, fail:0, na:0, err:true }; });
      })).then(function(){ render(); next(); });
    }
    next();
  },
  histExport:function(){
    var H = S.hist, rows = histFiltered(H);
    function q(v){ v = String(v==null?'':v); return /[",;\n]/.test(v) ? '"'+v.replace(/"/g,'""')+'"' : v; }
    var head = ['Дата','Время','Инженер','Приёмка/ремонт','Серийный номер','Этап','Модель','Режим','Пройдено','Ошибок','Не применимо','Файл'];
    var lines = [head.join(';')].concat(rows.map(function(x){
      var d = histLabel(x), m = H.sum[x.path] || {}, st = histStage(x);
      return [x.date, /^\d{6}/.test(x.file) ? x.file.slice(0,2)+':'+x.file.slice(2,4)+':'+x.file.slice(4,6) : '', x.engineer, d.intake, d.serial, st==='before'?'до':st==='after'?'после':'', m.model, m.mode, m.pass, m.fail, m.na, x.path].map(q).join(';');
    }));
    invoke('save_csv', { name:'history', content:lines.join('\r\n') }).then(function(path){
      H.err = ''; H.msg = 'Список сохранён: '+path; render(); invoke('open_containing_folder', { path:path }).catch(function(){});
    }).catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось сохранить список'; render(); });
  },
  evLoad:function(){
    var E = S.events = S.events || { date:evLocalDay(), list:null, loading:false, err:'' };
    E.loading = true; E.err = ''; render();
    invoke('srv_events', { date:E.date }).then(function(l){ E.list = l || []; }).catch(function(e){ E.err = typeof e==='string' ? e : 'Не удалось загрузить журнал'; E.list = []; })
      .then(function(){ E.loading = false; render(); });
  },
  evDate:function(v){ if (/^\d{4}-\d{2}-\d{2}$/.test(v)){ (S.events = S.events || {}).date = v; A.evLoad(); } },
  histBack:function(){ S.hist.view = null; S.hist.cmp = null; S.hist.open = {}; S.hist.saved = null; render(); },
  /* Раскрыть/свернуть подробности теста в открытом отчёте (строки лога из отчёта) */
  histToggle:function(id){ var H = S.hist; H.open = H.open || {}; H.open[id] = !H.open[id]; render(); },
  /* Скачать открытый отчёт: PDF / JSON / TXT (тот же формат, что «Экспорт» после автопрогона) */
  histSave:function(kind){
    var H = S.hist; if (!H.view || H.saving) return;
    var cmd = kind==='pdf' ? 'save_report_pdf' : kind==='json' ? 'save_report_json' : 'save_report_txt';
    H.saving = kind; H.err = ''; render();
    invoke(cmd, { report:H.view.env.report }).then(function(path){ H.saved = { kind:kind, path:path }; })
      .catch(function(e){ H.err = typeof e==='string' ? e : 'Не удалось сохранить отчёт'; })
      .then(function(){ H.saving = ''; render(); });
  },
  histReveal:function(){ if (S.hist.saved) invoke('open_containing_folder', { path:S.hist.saved.path }).catch(function(){}); },
  batLiveToggle:function(){
    var B = S.batLive;
    B.on = !B.on; clearTimeout(B.t);
    if (B.on){
      (function tick(){
        if (!B.on || S.screen!=='test' || S.cat!=='bat'){ B.on=false; return; }
        invoke('get_battery_live').then(function(d){ B.data=d; }).catch(function(){ B.data=null; }).then(function(){
          var el=document.getElementById('bat-live'); if (el) el.innerHTML = batLiveHtml();
          if (B.on) B.t = setTimeout(tick, 2000);
        });
      })();
    }
    render();
  },
  memPasses:function(v){ if(!S.mem.running){ S.mem.passes=v; render(); } },
  memStart:function(){
    if (S.mem.running) return;
    var m = S.mem; m.running=true; m.pct=0; m.pass=1; m.pattern=''; m.errors=0; m.res=null; m.err=null; m.t0=Date.now(); render();
    var unlisten=null;
    tauriEvent.listen('mem-progress', function(ev){
      var p=ev.payload; m.pct=p.pct; m.pass=p.pass; m.pattern=p.pattern; m.errors=p.errors;
      var f=document.getElementById('mem-fill'), t=document.getElementById('mem-txt');
      var el=(Date.now()-m.t0)/1000, eta = m.pct>1 ? el/m.pct*(100-m.pct) : null;
      var etaTxt = eta!=null ? ' · осталось ~'+Math.max(0,Math.round(eta))+' с' : '';
      if (f && t){ f.style.width=m.pct+'%'; t.textContent=m.pct+'% · проход '+m.pass+' · '+m.pattern+' · ошибок '+m.errors+etaTxt; } else render();
    }).then(function(u){ unlisten=u; });
    function fin(){ if(unlisten) unlisten(); m.running=false; }
    invoke('run_memory_test', { sizeMb:m.size, passes:m.passes }).then(function(r){
      fin(); m.res=r; render();
      var totalMb = S.hw && S.hw.ram_total_gb ? Math.round(S.hw.ram_total_gb*1024) : null;
      recordDetail('mem', { lines:['Проверено '+r.tested_mb+' МБ'+(totalMb?' из '+totalMb+' МБ установленной ОЗУ':'')+', проходов '+r.passes+', время '+r.elapsed_secs+' с'+' (проверяется вся свободная ОЗУ — остальное занято системой и другими процессами)'+(r.stopped?' (остановлено)':''),'Ошибок: '+r.errors].concat(r.first_errors) });
      if (S.auto.on && S.cat==='mem'){
        A.autoApply(r.stopped ? null
          : r.errors>0 ? { status:'fail', note:'Ошибок памяти: '+r.errors+' на '+r.tested_mb+' МБ — модуль или слот неисправны' }
          : { status:'pass', note:'Ошибок нет: проверено '+r.tested_mb+' МБ'+(totalMb?' из '+totalMb+' МБ':'')+' за '+r.elapsed_secs+' с'+'' });
      }
    }).catch(function(err){
      fin(); m.err = typeof err==='string'?err:'Ошибка теста памяти'; render();
      if (S.auto.on && S.cat==='mem') A.autoApply(null);
    });
  },
  memStop:function(){ invoke('stop_memory_test').catch(function(){}); },

  /* ---- яркость ---- */
  brLoad:function(){
    if (S.br.info || S.br.loading) return;
    S.br.loading = true;
    invoke('get_brightness').then(function(r){
      S.br.info = r; S.br.loading = false; render();
      if (!r.available && S.auto.on && S.cat==='bright') A.autoApply({ status:'na', note:'Управление яркостью недоступно (настольный ПК или внешний монитор)' });
    }).catch(function(err){
      S.br.info = { available:false, current:0, min:0, max:100, error: typeof err==='string'?err:'' }; S.br.loading=false; render();
      if (S.auto.on && S.cat==='bright') A.autoApply({ status:'na', note:'Управление яркостью недоступно' });
    });
  },
  brInput:function(v){
    var l = document.getElementById('br-val'); if (l) l.textContent = v+'%';
    clearTimeout(S.brT);
    S.brT = setTimeout(function(){ invoke('set_brightness', { level:parseInt(v,10) }).catch(function(){}); }, 120);
  },
  brSet:function(v){ if (S.br.info) S.br.info.current=v; invoke('set_brightness', { level:v }).catch(function(){}); render(); },

  /* ---- USB-накопитель ---- */
  rmStart:function(){
    if (S.rm.timer) return;
    function poll(){
      if (S.rm.running) return;
      invoke('list_removable_drives').then(function(list){
        var changed = JSON.stringify(list) !== JSON.stringify(S.rm.drives);
        S.rm.drives = list; S.rm.err = null;
        if (changed && S.screen==='test' && cat().kind==='removable') render();
      }).catch(function(err){
        S.rm.err = typeof err==='string' ? err : 'Не удалось получить список накопителей';
        if (S.rm.drives===null){ S.rm.drives = []; render(); }
      });
    }
    poll(); S.rm.timer = setInterval(poll, 2000);
  },
  rmTest:function(letter){
    if (S.rm.running) return;
    S.rm.running = letter; render();
    invoke('test_removable_drive', { letter:letter, sizeMb:S.rm.size }).then(function(r){
      S.rm.running = null; S.rm.log.unshift(r);
      // в отчёт: скорость и класс порта по каждой проверке
      recordDetail(S.cat, { lines: S.rm.log.filter(function(x){ return !x.error; }).reverse().map(function(x){
        return x.letter+': запись '+x.write_mbps.toFixed(1)+' МБ/с, чтение '+x.read_mbps.toFixed(1)+' МБ/с, '+(x.errors?'ошибок данных: '+x.errors:'данные совпали')+' — '+usbClass(x).txt;
      }) });
      render();
    }).catch(function(err){
      S.rm.running = null; S.rm.log.unshift({ letter:letter, error: typeof err==='string'?err:'Ошибка теста накопителя' }); render();
    });
  },

  /* ---- запись с камеры ---- */
  camRecord:function(){
    if (!S.camStream || S.camRecording || typeof MediaRecorder==='undefined') return;
    var chunks = [], rec = new MediaRecorder(S.camStream);
    S.camRecording = true; S.camClip = null; render();
    rec.ondataavailable = function(e){ if (e.data.size) chunks.push(e.data); };
    rec.onstop = function(){
      S.camRecording = false;
      if (chunks.length) S.camClip = URL.createObjectURL(new Blob(chunks, { type: rec.mimeType }));
      render();
    };
    rec.start();
    setTimeout(function(){ if (rec.state!=='inactive') rec.stop(); }, 5000);
  },

  /* ---- runner-категории: реальные invoke-запросы ---- */
  run:function(){
    var c = cat();
    S.running = true; S.runLines=[]; S.runError=null; render();
    S.verdict = null;
    fetchCategory(c.fetch).then(function(res){
      S.running=false; S.runLines=res.lines; S.verdict=res.verdict; S.runActions=res.actions||[]; render();
      recordDetail(c.id, { lines: res.lines.slice(0,200), auto: res.verdict || null });
      if (c.fetch==='winact' && res.verdict && res.verdict.status==='fail' && !S.act.autoFixDone){
        A.winactAutoFix(); return;
      }
      if (S.auto.on && S.cat===c.id) A.autoApply(res.verdict);
    }).catch(function(err){
      S.running=false; S.runError = typeof err==='string' ? err : 'Ошибка получения данных';
      render();
      if (S.auto.on && S.cat===c.id) A.autoApply(null);
    });
  },
  /* Активация не пройдена — сама пробует штатные шаги устранения по очереди
     (синхронизация времени → перезапуск службы → онлайн-активация → ключ
     из BIOS, если есть), без подтверждения на каждый шаг: это те же самые
     действия, что и ручные кнопки ниже, техник просто не должен нажимать их
     по одной каждый раз. Ввод стороннего ключа — только вручную. */
  winactAutoFix:function(){
    if (S.cat!=='winact') return;
    S.act.autoFixDone = true;
    S.act.autoFixRunning = true;
    S.running = true;
    var lines = S.runLines.concat(['— Активация не пройдена, пробую штатные шаги устранения —']);
    S.runLines = lines; render();
    var LABELS = { sync_time:'Синхронизация времени', restart_service:'Перезапуск службы лицензирования', activate:'Онлайн-активация', install_oem_key:'Установка OEM-ключа из BIOS', settings_troubleshoot:'Кнопка «Устранение неполадок» в Параметрах Windows' };
    var steps = ['sync_time', 'restart_service', 'activate'];
    if (S.actRaw && S.actRaw.oem_key_present) steps.push('install_oem_key');
    // Последним — то же, что техник делает руками в Параметрах → Активация.
    steps.push('settings_troubleshoot');
    function addLine(t){ lines = lines.concat([t]); S.runLines = lines; render(); }
    function runStep(i){
      if (S.cat!=='winact'){ return; }
      if (i >= steps.length) return finish();
      var step = steps[i];
      addLine(LABELS[step] + '…');
      invoke('run_activation_step', { step: step, key: null }).then(function(r){
        addLine('    ' + r);
        if (step==='settings_troubleshoot') return pollAfterTroubleshoot(i);
        afterStep(i);
      }).catch(function(err){
        addLine('    не выполнено: ' + (typeof err==='string' ? err : 'ошибка'));
        afterStep(i);
      });
    }
    // Средство Windows отрабатывает не мгновенно — ждём активации до ~90 с.
    function pollAfterTroubleshoot(i){
      var tries = 0;
      (function tick(){
        if (S.cat!=='winact'){ return; }
        invoke('get_activation_status').then(function(a){
          S.actRaw = a;
          if (a.found && a.license_status===1){ addLine('    Windows активирована'); return finish(); }
          if (++tries >= 18){ addLine('    за 90 с активация не подтвердилась'); return afterStep(i); }
          setTimeout(tick, 5000);
        }).catch(function(){
          if (++tries >= 18) return afterStep(i);
          setTimeout(tick, 5000);
        });
      })();
    }
    function afterStep(i){
      invoke('get_activation_status').then(function(a){
        S.actRaw = a;
        if (a.found && a.license_status===1) return finish();
        runStep(i + 1);
      }).catch(function(){ runStep(i + 1); });
    }
    function finish(){
      if (S.cat!=='winact'){ return; }
      S.act.autoFixRunning = false;
      fetchCategory('winact').then(function(res){
        S.running = false;
        S.runLines = lines.concat(['— Итог после автоустранения —']).concat(res.lines);
        S.verdict = res.verdict; S.runActions = res.actions||[]; render();
        recordDetail('winact', { lines: S.runLines.slice(0,200), auto: res.verdict || null });
        if (S.auto.on && S.cat==='winact') A.autoApply(res.verdict);
      }).catch(function(){
        S.running = false; render();
        if (S.auto.on && S.cat==='winact') A.autoApply(null);
      });
    }
    runStep(0);
  },

  /* ---- камера ---- */
  camStart:function(){
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia){
      S.runError='Камера недоступна в этом окне (getUserMedia не поддерживается webview).'; render(); return;
    }
    // Камеры нет вообще — в автопрогоне сами ставим «не применимо» и идём дальше (ноутбук: напоминаем проверить)
    function noCamera(){
      var lap = S.hw && S.hw.is_laptop;
      var note = 'Камера не обнаружена в системе — не применимо' + (lap ? ' (на ноутбуке проверьте, должна ли она быть)' : '');
      S.runError = 'Камера не обнаружена в системе. Отметьте «Не применимо».';
      if (S.auto.on && S.cat==='cam') A.autoApply({ status:'na', note:note }); else render();
    }
    var enumP = navigator.mediaDevices.enumerateDevices ? navigator.mediaDevices.enumerateDevices().catch(function(){ return null; }) : Promise.resolve(null);
    enumP.then(function(list){
      if (list && !list.some(function(d){ return d.kind==='videoinput'; })){ noCamera(); return; }
      navigator.mediaDevices.getUserMedia({ video:true }).then(function(stream){
        S.camStream = stream; render();
        var v = document.getElementById('cam-preview');
        if (v) v.srcObject = stream;
      }).catch(function(err){
        if (err && (err.name==='NotFoundError' || err.name==='DevicesNotFoundError')){ noCamera(); return; }
        S.runError = 'Нет доступа к камере: ' + (err && err.message ? err.message : err);
        render();
      });
    });
  },
  /* Тачпад: на настольном ПК (не ноутбук и нет батареи) его нет — в автопрогоне ставим «не применимо» и идём дальше.
     На ноутбуках тест всегда идёт вручную: тачпад там должен быть, его отсутствие — возможная неисправность. */
  padProbe:function(c){
    if (!S.auto.on) return;
    invokeCached('get_battery_info', {}, 30000).then(function(b){
      var laptop = (S.hw && S.hw.is_laptop) || (b && b.present);
      if (!laptop && S.auto.on && S.cat===c.id) A.autoApply({ status:'na', note:'Настольный ПК (нет батареи) — тачпада нет, не применимо' });
    }).catch(function(){});
  },

  /* ---- звук ---- */
  tone:function(i){
    stopAudio();
    S.tone=i;
    render();
    try{
      var Ctx = window.AudioContext || window.webkitAudioContext;
      S.toneCtx = new Ctx();
      S.toneAnalyser = S.toneCtx.createAnalyser();
      S.toneAnalyser.fftSize = 128;
      if (i===2){
        navigator.mediaDevices.getUserMedia({ audio:true }).then(function(stream){
          S.toneMic = stream;
          var src = S.toneCtx.createMediaStreamSource(stream);
          src.connect(S.toneAnalyser);
        }).catch(function(err){
          S.runError='Нет доступа к микрофону: ' + (err && err.message ? err.message : err);
          render();
        });
      } else {
        S.toneOsc = S.toneCtx.createOscillator();
        S.toneOsc.frequency.value = 1000;
        var gain = S.toneCtx.createGain();
        gain.gain.value = 0.15;
        if (i===1){
          var panner = S.toneCtx.createStereoPanner ? S.toneCtx.createStereoPanner() : null;
          S.toneOsc.connect(gain);
          if (panner){ gain.connect(panner); panner.connect(S.toneAnalyser); panner.connect(S.toneCtx.destination); }
          else { gain.connect(S.toneAnalyser); gain.connect(S.toneCtx.destination); }
        } else {
          S.toneOsc.connect(gain); gain.connect(S.toneAnalyser); gain.connect(S.toneCtx.destination);
        }
        S.toneOsc.start();
      }
    } catch(e){ S.runError='Web Audio API недоступен: ' + e.message; render(); }
  },

  /* ---- датчики ---- */
  sensorsStart:function(){
    stopSensorPoll();
    A.hwmRefresh();
    function poll(){
      invoke('hwmon_snapshot').then(function(sn){ S.hwm.snap = sn; var gt = gpuHottest(sn); if (gt!=null && !(S.sensorReading && S.sensorReading.gpu)) S.gpuHistory = S.gpuHistory.concat([gt]).slice(-100); }).catch(function(){});
      invoke('get_thermal_reading').then(function(r){
        S.sensorReading = r;
        if (r.available && r.cpu_temp_c!=null){
          S.sensorHistory = S.sensorHistory.concat([r.cpu_temp_c]).slice(-100);
        }
        if (r.gpu){ S.gpuHistory = S.gpuHistory.concat([r.gpu.temp_c]).slice(-100); }
        render();
      }).catch(function(err){
        S.sensorReading = { available:false, cpu_temp_c:null, note: typeof err==='string'?err:'Ошибка опроса датчиков' };
        render();
      });
    }
    poll();
    S.sensorPoll = setInterval(poll, 2000);
  },
  snapshot:function(){ S.snapshot=true; render(); },

  /* ---- вентиляторы (как в SpeedFan) ---- */
  fanStart:function(){
    if (S.fan.poll) return;
    invoke('hwmon_start').catch(function(){});
    function tick(){
      invoke('hwmon_snapshot').then(function(sn){ S.hwm.snap = sn; noteFans(sn); paintFans(); }).catch(function(){});
    }
    tick(); S.fan.poll = setInterval(tick, 1000);
  },
  fanManual:function(cid, v){
    var f = S.fan; f.manual[cid] = parseInt(v,10); f.touched = true;
    var l = document.getElementById('fan-pct-'+cssId(cid)); if (l) l.textContent = v+'%';
    clearTimeout(f.setT); f.setT = setTimeout(function(){ invoke('hwmon_fan_set', { id:cid, percent:parseInt(v,10) }).catch(function(){}); }, 150);
    // процесс сам вернёт авторежим через 40 с — повторяем команду, пока экран открыт
    if (!f.refresh) f.refresh = setInterval(function(){
      Object.keys(f.manual).forEach(function(id){ invoke('hwmon_fan_set', { id:id, percent:f.manual[id] }).catch(function(){}); });
    }, 10000);
  },
  fanAuto:function(){
    var f = S.fan; f.manual = {}; f.touched = false;
    invoke('hwmon_fan_default_all').catch(function(){}); render();
  },
  fanStop:function(){ S.fan.abort = true; },
  fanTest:function(active){
    var f = S.fan; if (f.running) return;
    f.running = true; f.abort = false; f.log = []; f.res = null; f.manual = {};
    if (S.screen==='test') render();
    function log(t){ f.log.push(t); paintFans(); }
    function sleep(ms){ return new Promise(function(r){ setTimeout(r, ms); }); }
    function finish(v){
      f.running = false; f.res = v;
      recordDetail('fans', { lines: f.log.slice(0,80), auto: v });
      invoke('hwmon_fan_default_all').catch(function(){});
      if (S.screen==='test') render();
      if (S.auto.on && S.cat==='fans') A.autoApply(v);
    }
    async function snapOk(){
      var sn = null;
      for (var i=0; i<8 && !(sn && sn.ok); i++){
        sn = await invoke('hwmon_snapshot').catch(function(){ return null; });
        if (!(sn && sn.ok)) await sleep(1000);
      }
      if (sn && sn.ok) S.hwm.snap = sn;
      return sn && sn.ok ? sn : null;
    }
    async function sample(fanId, secs){
      var out = [];
      for (var i=0; i<secs && !f.abort; i++){
        await sleep(1000);
        var sn = await invoke('hwmon_snapshot').catch(function(){ return null; });
        if (sn && sn.ok){ S.hwm.snap = sn; var x = sn.sensors.filter(function(s){ return s.id===fanId; })[0]; if (x){ out.push(x.value); noteFans(sn); paintFans(); } }
      }
      return out;
    }
    (async function(){
      try {
        await invoke('hwmon_start').catch(function(){});
        var sn = await snapOk();
        if (!sn){ log('Датчики не отвечают — нужен драйвер PawnIO (вкладка «Датчики»).'); return finish({ status:'na', note:'Датчики недоступны (драйвер PawnIO не установлен или не запущен)' }); }
        var fans = fanList(sn);
        if (!fans.length){
          // Различаем «драйвер датчиков работает, но тахометра нет» и «датчиков нет совсем»:
          // в первом случае вентилятор у ноутбука может быть, просто контроллер EC не отдаёт обороты.
          var temps = sn.sensors.filter(function(x){ return x.type==='Temperature'; }).length;
          var why = temps
            ? 'Датчики работают (температур: '+temps+'), но тахометра вентилятора нет: контроллер EC этой модели не отдаёт обороты (это не значит, что вентилятора нет).'
            : 'Датчики температуры и оборотов не найдены.';
          log(why);
          // Тахометра нет — в автопрогоне оцениваем охлаждение косвенно: 60 с нагрузки на CPU,
          // температура и падение частоты (то же правило, что у стресс-теста).
          if (temps && S.auto.on && S.auto.ids.indexOf('stress')>=0){
            log('Охлаждение оценено стресс-тестом автопрогона (нагрузка 2 мин) — см. результат теста «Стресс-тест».');
            return finish({ status:'na', note:'Обороты не читаются (EC не отдаёт тахометр); охлаждение оценено стресс-тестом автопрогона' });
          }
          if (temps && S.auto.on && !S.st.running && !f.abort){
            log('Прогрев CPU 60 с: оцениваем охлаждение по температуре под нагрузкой…');
            var warm = await new Promise(function(resolve){
              var unl = [], temps30 = [], settled = false;
              function fin(res){ if (settled) return; settled = true; unl.forEach(function(u){ u(); }); resolve({ res:res, t:temps30 }); }
              tauriEvent.listen('stress-tick', function(ev){ if (ev.payload.tempC!=null) temps30.push(ev.payload.tempC); }).then(function(u){ unl.push(u); });
              tauriEvent.listen('stress-done', function(ev){ fin(ev.payload); }).then(function(u){ unl.push(u); });
              invoke('start_stress', { cfg:{ durationSecs:60, cpu:true, fpu:true, cache:false, memory:false, disk:false, gpu:false, threads:0,
                memoryPercent:50, diskLetter:'', diskMb:1024, maxTempC: profile().maxTempC || 95 } }).catch(function(){ fin(null); });
              var guard = setInterval(function(){ if (f.abort){ invoke('stop_stress').catch(function(){}); clearInterval(guard); } if (settled) clearInterval(guard); }, 1000);
            });
            if (warm.res){
              var t0 = warm.t.length ? warm.t[0] : null, tMax = warm.t.length ? Math.max.apply(null, warm.t) : null;
              log('Прогрев завершён: температура '+(t0!=null ? t0.toFixed(0)+' → макс '+tMax.toFixed(0)+' °C' : 'н/д')+', загрузка CPU '+warm.res.avgLoad.toFixed(0)+'%'+(warm.res.clockAvgMhz ? ', частота '+warm.res.clockMinMhz.toFixed(0)+'–'+warm.res.clockMaxMhz.toFixed(0)+' МГц' : ''));
              var jv = warm.res.reason==='thermal'
                ? { status:'fail', note:'Прогрев 60 с остановлен температурной защитой — перегрев (обороты вентилятора датчиками недоступны)' }
                : (t0==null ? null : judgeStress(warm.res));
              if (jv) return finish({ status:jv.status, note:'Обороты не читаются; охлаждение по прогреву 60 с: '+jv.note });
            }
          }
          log('Оценить охлаждение можно по температуре под нагрузкой (вкладка «Стресс-тест») и на слух.');
          return finish({ status:'na', note: temps ? 'Обороты вентилятора недоступны: EC не отдаёт тахометр (температуры читаются, оценивайте по стресс-тесту)' : 'Датчики оборотов и температур не найдены' });
        }
        log('Найдено вентиляторов: '+fans.length);
        fans.forEach(function(x){ log('  • '+x.name+' ('+x.hw+'): '+x.rpm.toFixed(0)+' об/мин'+(x.control ? ', управляется ('+x.control.pct.toFixed(0)+'%)' : ', без ручного управления')); });
        var cpuT = cpuTempFromSnap(sn), problems = [], notes = [];
        for (var i=0; i<fans.length && !f.abort; i++){
          var fan = fans[i];
          if (active && fan.control){
            log('Проверка «'+fan.name+'»: 100% …');
            await invoke('hwmon_fan_set', { id:fan.control.id, percent:100 }).catch(function(){});
            var hi = await sample(fan.id, 8); var rpmHi = hi.length ? Math.max.apply(null, hi) : 0;
            log('  100% → '+rpmHi.toFixed(0)+' об/мин');
            log('Проверка «'+fan.name+'»: 35% …');
            await invoke('hwmon_fan_set', { id:fan.control.id, percent:35 }).catch(function(){});
            var lo = await sample(fan.id, 8); var rpmLo = lo.length ? lo[lo.length-1] : 0;
            log('  35% → '+rpmLo.toFixed(0)+' об/мин');
            await invoke('hwmon_fan_default', { id:fan.control.id }).catch(function(){});
            if (rpmHi < 500) problems.push('«'+fan.name+'» на 100% только '+rpmHi.toFixed(0)+' об/мин (не раскручивается)');
            else if (rpmHi < rpmLo*1.15) problems.push('«'+fan.name+'» не реагирует на управление (100%: '+rpmHi.toFixed(0)+', 35%: '+rpmLo.toFixed(0)+' об/мин)');
            else notes.push(fan.name+': '+rpmLo.toFixed(0)+' → '+rpmHi.toFixed(0)+' об/мин');
          } else {
            var rr = await sample(fan.id, 3); var cur = rr.length ? rr[rr.length-1] : fan.rpm;
            log('«'+fan.name+'»: '+cur.toFixed(0)+' об/мин (пассивная проверка)');
            if (cur<=0){ if (cpuT!=null && cpuT>=65) problems.push('«'+fan.name+'» стоит при температуре CPU '+cpuT.toFixed(0)+' °C'); else notes.push(fan.name+': остановлен (в простое это бывает нормально)'); }
            else notes.push(fan.name+': '+cur.toFixed(0)+' об/мин');
          }
        }
        if (f.abort) { log('Проверка остановлена.'); return finish(null); }
        finish(problems.length ? { status:'fail', note:problems.join('; ') }
          : { status:'pass', note:'Вентиляторов: '+fans.length+' — '+notes.join('; ')+(active ? '' : ' (пассивная проверка; для проверки отклика запустите активный тест)') });
      } catch(e){ log('Ошибка: '+(e && e.message ? e.message : e)); finish(null); }
    })();
  },

  /* ---- датчики LibreHardwareMonitor и драйвер PawnIO ---- */
  hwmRefresh:function(){
    invoke('hwmon_status').then(function(st){
      S.hwm.status = st;
      var noAuto = false; try { noAuto = localStorage.getItem('echips_no_auto_pawnio')==='1'; } catch(e){}
      if (st.embedded && st.driverEmbedded && !st.driverInstalled && !S.hwm.autoTried && !S.hwm.busy && !noAuto){
        // приложение для инженеров: драйвер ставится сам, без вопросов; вручную можно удалить
        S.hwm.autoTried = true; S.hwm.confirm = 'install'; A.hwmRun(); return;
      }
      if (st.embedded && st.driverInstalled && !st.running) invoke('hwmon_start').catch(function(){});
      if (S.screen==='sensors') render();
    }).catch(function(){});
  },
  hwmStart:function(){ invoke('hwmon_start').then(function(){ A.hwmRefresh(); }).catch(function(err){ S.hwm.err = typeof err==='string'?err:'Не удалось запустить датчики'; render(); }); },
  hwmAsk:function(step){ S.hwm.confirm = step; S.hwm.msg=''; S.hwm.err=''; render(); },
  hwmInstall:function(){ try { localStorage.removeItem('echips_no_auto_pawnio'); } catch(e){} S.hwm.autoTried = true; S.hwm.confirm = 'install'; S.hwm.msg=''; S.hwm.err=''; A.hwmRun(); },
  hwmCancel:function(){ S.hwm.confirm = null; render(); },
  hwmRun:function(){
    var step = S.hwm.confirm; if (!step || S.hwm.busy) return;
    S.hwm.busy = true; S.hwm.err=''; render();
    if (step==='uninstall'){ try { localStorage.setItem('echips_no_auto_pawnio','1'); } catch(e){} }
    invoke(step==='uninstall' ? 'hwmon_uninstall_driver' : 'hwmon_install_driver').then(function(r){
      S.hwm.busy = false; S.hwm.confirm = null; S.hwm.msg = r; S.hwm.snap = null;
      A.hwmRefresh();
    }).catch(function(err){
      S.hwm.busy = false; S.hwm.confirm = null; S.hwm.err = typeof err==='string' ? err : 'Не удалось выполнить действие'; render();
    });
  },

  /* ---- стресс-тест ---- */
  stToggle:function(k){ if (S.st.running) return; S.st.cfg[k] = !S.st.cfg[k]; render(); },
  stSet:function(k,v){ if (S.st.running) return; S.st.cfg[k] = v; render(); },
  stPreset:function(name){
    if (S.st.running) return;
    var p = ST_PRESETS[name], c = S.st.cfg;
    ['cpu','fpu','cache','memory','disk','gpu'].forEach(function(k){ c[k] = !!p[k]; });
    c.dur = p.dur; render();
  },
  stClear:function(){ if (S.st.running) return; S.st.res=null; S.st.err=null; S.st.events=[]; S.st.last=null; S.st.elapsed=0; S.st.hist={ load:[], temp:[], gpuT:[], clock:[], clockMax:0, scores:{} }; render(); },
  stStop:function(){ if (S.st.running) invoke('stop_stress').catch(function(){}); },
  stStart:function(){
    var st = S.st, c = st.cfg; if (st.running) return;
    if (!(c.cpu||c.fpu||c.cache||c.memory||c.disk||c.gpu)){ st.err='Выберите хотя бы один вид нагрузки'; render(); return; }
    var hwThreads = (S.hw && S.hw.cpu && S.hw.cpu.threads) || 2;
    var cfg = { durationSecs:c.dur, cpu:c.cpu, fpu:c.fpu, cache:c.cache, memory:c.memory, disk:c.disk, gpu:c.gpu,
      threads: c.threads==='half' ? Math.max(1, Math.floor(hwThreads/2)) : c.threads==='one' ? 1 : 0,
      memoryPercent:c.memPct, diskLetter:'', diskMb:1024, maxTempC: profile().maxTempC || 95 };
    st.running=true; st.err=null; st.res=null; st.events=[]; st.last=null; st.elapsed=0; st.gpuFps=null;
    st.hist = { load:[], temp:[], gpuT:[], clock:[], clockMax:0, scores:{} };
    render();
    var unl = [];
    function cleanup(){ unl.forEach(function(u){ u(); }); gpuStressStop(); }
    tauriEvent.listen('stress-tick', function(ev){ stOnTick(ev.payload); }).then(function(u){ unl.push(u); });
    tauriEvent.listen('stress-done', function(ev){ cleanup(); stOnDone(ev.payload); }).then(function(u){ unl.push(u); });
    if (c.gpu) gpuStressStart();
    invoke('hwmon_start').catch(function(){});
    invoke('start_stress', { cfg:cfg }).catch(function(err){
      cleanup(); st.running=false; st.err = typeof err==='string' ? err : 'Не удалось запустить стресс-тест'; render();
      if (S.auto.on && S.cat==='stress') A.autoApply(null);
    });
  },
  /* Прерванный стресс-тест (перезагрузка/зависание): запись в отчёт или закрытие уведомления */
  markerRecord:function(){
    var m = S.stressMarker; if (!m) return;
    var note = 'Стресс-тест прерван на '+m.lastElapsed+' с — перезагрузка, зависание или выключение питания посреди теста. Последние показания: загрузка '+Math.round(m.lastLoad)+'%, температура '+(m.lastTempC!=null ? m.lastTempC.toFixed(0)+' °C' : 'н/д')+(m.lastGpuTempC!=null ? ', GPU '+m.lastGpuTempC.toFixed(0)+' °C' : '');
    S.results.stress = 'fail'; S.comments.stress = note;
    recordDetail('stress', { auto:{ status:'fail', note:note }, final:'fail', lines:[note, 'Нагрузки: '+m.stressors.join(', ')+' · запущен '+m.startedAt+' · заданная длительность '+(m.durationSecs||'до остановки')] });
    invoke('clear_stress_marker').catch(function(){}); S.stressMarker = null; renderNav(); render();
  },
  /* Продолжить прерванный (например, 4-часовой) прогон: те же нагрузки на оставшееся время; прерывание остаётся в журнале событий.
     Автозапуск после перезагрузки не делаем (это правка автозагрузки Windows) — инженер запускает приложение сам. */
  markerResume:function(){
    var m = S.stressMarker; if (!m) return;
    var rem = Math.max(60, (m.durationSecs||0) - (m.lastElapsed||0));
    invoke('clear_stress_marker').catch(function(){}); S.stressMarker = null;
    A.go('stress');
    var c = S.st.cfg, names = m.stressors||[];
    ['cpu','fpu','cache','memory','disk','gpu'].forEach(function(k){ c[k] = names.indexOf(k)>=0; });
    c.dur = rem; c.threads = 'all';
    A.stStart();
    S.st.events.push('Продолжение прерванного прогона (прервано на '+m.lastElapsed+' с из '+m.durationSecs+' с)');
  },
  markerDismiss:function(){ invoke('clear_stress_marker').catch(function(){}); S.stressMarker = null; render(); },

  /* ---- установка драйверов ---- */
  drvStart:function(){
    S.drv = { step:'scan', scanLabel:'СКАНИРОВАНИЕ ОБОРУДОВАНИЯ', restore:true };
    render();
    invokeCached('get_system_info', {}, 600000).then(function(info){
      S.device = info; render();
      S.drv.scanLabel = 'ПРОВЕРКА БАЗЫ ДРАЙВЕРОВ'; render();
      return invoke('fetch_public_json', { publicUrl: MANIFEST_PUBLIC_URL }).catch(function(){
        return invoke('load_cached_manifest');
      });
    }).then(function(manifest){
      S.drv.manifest = manifest;
      invoke('cache_manifest', { manifest: manifest }).catch(function(){});
      return invoke('find_by_name', { manifest: manifest, manufacturer: S.device.manufacturer, model: S.device.model });
    }).then(function(match){
      if (match) return match;
      return invoke('find_by_serial_prefix', { manifest: S.drv.manifest, serial: S.device.serial_number });
    }).then(function(match){
      S.drv.autoKey = match ? match[0] : null;
      return invoke('list_problem_devices').catch(function(){ return []; });
    }).then(function(problems){
      S.drv.problems = problems;
      S.drv.groups = buildModelGroups(S.drv.manifest, S.drv.autoKey);
      S.drv.pickIdx = 0;
      S.drv.step = 'pick'; render();
    }).catch(function(err){
      S.drv.step='error'; S.drv.backTo='start';
      S.drv.error = typeof err==='string' ? err : 'Не удалось получить каталог драйверов. Проверьте подключение к интернету.';
      render();
    });
  },
  drvSearch:function(v){
    var q = String(v).trim().toLowerCase();
    Array.prototype.forEach.call(document.querySelectorAll('#model-list .modelrow'), function(row){
      row.style.display = (!q || row.getAttribute('data-search').indexOf(q)>=0) ? '' : 'none';
    });
  },
  drvPick:function(i){ S.drv.pickIdx = i; },
  drvSelect:function(){
    var g = S.drv.groups[S.drv.pickIdx]; if(!g) return;
    var key = g.keys.indexOf(S.drv.autoKey)>=0 ? S.drv.autoKey : g.keys[0];
    S.drv.entry = { key:key, entry:S.drv.manifest[key], name:g.name };
    S.drv.mode = 'model'; S.drv.step = 'confirm'; render();
  },
  drvUniversal:function(){
    var uni = S.drv.manifest && S.drv.manifest['_universal'];
    if(!uni){ S.drv.step='error'; S.drv.backTo='pick'; S.drv.error='Универсальный набор драйверов недоступен.'; render(); return; }
    S.drv.step='scan'; S.drv.scanLabel='ЗАГРУЗКА СПИСКА КАТЕГОРИЙ'; render();
    invoke('yandex_list_folder', { publicKey: uni.yandex_public_key }).then(function(files){
      if(!files.length){ throw 'В универсальном наборе пока нет ни одного пакета драйверов.'; }
      S.drv.uniFiles = files; S.drv.uniPicked = {};
      var classes = {};
      (S.drv.problems||[]).forEach(function(d){ if(d['class']) classes[d['class'].toLowerCase()] = true; });
      files.forEach(function(f,i){
        Object.keys(classes).forEach(function(c){ if(f[0].toLowerCase().indexOf(c)>=0) S.drv.uniPicked[i]=true; });
      });
      S.drv.step='universal'; render();
    }).catch(function(err){
      S.drv.step='error'; S.drv.backTo='pick';
      S.drv.error = typeof err==='string' ? err : 'Не удалось получить список универсальных пакетов драйверов.';
      render();
    });
  },
  drvCat:function(i,on){ if(on) S.drv.uniPicked[i]=true; else delete S.drv.uniPicked[i]; },
  drvUniNext:function(){
    var files = S.drv.uniFiles.filter(function(f,i){ return S.drv.uniPicked[i]; });
    if(!files.length) return;
    S.drv.chosenFiles = files; S.drv.mode = 'universal'; S.drv.step='confirm'; render();
  },
  drvStep:function(step){ S.drv.step = step; render(); },
  drvRestore:function(v){ S.drv.restore=v; render(); },
  drvDetails:function(){ S.drv.showDetails = !S.drv.showDetails; render(); },
  drvInstall:function(){
    var d = S.drv;
    d.items = d.mode==='model' ? [d.entry.name] : d.chosenFiles.map(function(f){ return f[0].replace(/\.zip$/i,''); });
    d.doneIdx = {}; d.step='installing'; d.progress={ pct:0, label:'Подготовка...', mb:'' }; render();
    var unlisten = [];
    tauriEvent.listen('install-progress', function(ev){
      var p = ev.payload;
      if (p.stage==='downloading' && p.total>0){
        d.progress = { pct:Math.round(p.downloaded/p.total*100), label:'Загрузка '+p.file_label+'...',
          mb:(p.downloaded/1048576).toFixed(1)+' / '+(p.total/1048576).toFixed(1)+' МБ' };
      } else if (p.stage==='installing'){
        d.progress = { pct:100, label:p.file_label, mb:'' };
      } else {
        d.progress = { pct:d.progress.pct, label:p.file_label, mb:d.progress.mb };
      }
      // Без полной перерисовки — иначе сбрасывается анимация блика на прогресс-баре.
      var lb=document.getElementById('prog-label'), fl=document.getElementById('prog-fill'), mb=document.getElementById('prog-mb');
      if (lb && fl){ lb.textContent=d.progress.label; fl.style.width=d.progress.pct+'%'; if(mb) mb.textContent=d.progress.mb; }
      else render();
    }).then(function(u){ unlisten.push(u); });
    tauriEvent.listen('file-progress', function(ev){
      if (ev.payload.status==='done'){ d.doneIdx[ev.payload.index]=true; render(); }
    }).then(function(u){ unlisten.push(u); });

    var files = d.mode==='model'
      ? [[d.entry.entry.yandex_public_key, d.entry.entry.path || null, d.entry.name]]
      : d.chosenFiles.map(function(f){ return [S.drv.manifest['_universal'].yandex_public_key, f[1], f[0]]; });
    function done(){ unlisten.forEach(function(u){ u(); }); }
    invoke('download_and_install', { files: files, createRestore: d.restore }).then(function(result){
      done(); d.step='done'; d.result=result; render();
      notifyDone('Echips Hardware Check', result.message);
    }).catch(function(err){
      done(); d.step='error'; d.backTo='confirm';
      d.error = typeof err==='string' ? err : 'Ошибка установки'; render();
    });
  },

  /* ---- замена платы ----
     Отдельный PIN-шаг здесь убран (см. CLAUDE.md, задача №2) — техник уже
     определён общим экраном входа при запуске (S.engineer), спрашивать
     PIN второй раз для этой вкладки незачем. */
  mbReset:function(){
    S.mb = { step:'reading', techId:'', techName: S.engineer ? S.engineer.name : '', pin:'', pinErr:'', ticket:'', serial:'', uuid:'', formErr:'', before:null, writeError:null };
    render();
    invoke('read_board_identity').then(function(id){
      S.mb.before = id; S.mb.step='form'; render(); A.mbLoadSaved();
    }).catch(function(err){
      S.mb.pinErr = typeof err==='string'?err:'Не удалось прочитать SN/UUID платы';
      S.mb.step='readerror'; render();
    });
  },
  mbField:function(k,v){ S.mb[k]=v; },
  mbNext:function(){
    var ticket=(S.mb.ticket||'').trim(), serial=(S.mb.serial||'').trim(), uuid=(S.mb.uuid||'').trim();
    // SN и UUID можно писать раздельно: пустое поле = не менять
    if(!ticket){ S.mb.formErr='Укажите номер наряда.'; render(); return; }
    if(!serial && !uuid){ S.mb.formErr='Заполните серийный номер и/или UUID (пустое поле не меняется).'; render(); return; }
    if(serial && !isValidSerial(serial)){ S.mb.formErr='Серийный номер: 4–40 символов — латинские буквы, цифры, . _ - (не с дефиса).'; render(); return; }
    if(uuid && !isValidUuid(uuid)){ S.mb.formErr='UUID в формате 8-4-4-4-12.'; render(); return; }
    S.mb.serial = serial; S.mb.uuid = uuid;
    S.mb.formErr=''; S.mb.step='confirm'; render();
  },
  /* Одна кнопка после записи: сверка SN системы/платы, UUID, MAC и OEM-ключа — результат идёт в отчёт как тест «Идентификаторы» */
  mbVerify:function(){ A.go('test','ident'); A.run(); },
  /* Сохранение текущих SN/UUID в файл (папка открывается) и подстановка их из сохранённых файлов после замены платы */
  mbLoadSaved:function(){
    invoke('mb_list_identities').then(function(l){ S.mb.saved = l || []; render(); }).catch(function(){ S.mb.saved = []; });
  },
  mbSaveIdentity:function(){
    var m = S.mb; if (!m.before) return;
    m.idErr = ''; m.idMsg = '';
    invoke('mb_save_identity', { data:{ saved_at:new Date().toISOString(), serial:String(m.before.serial_number||'').trim(), uuid:String(m.before.uuid||'').trim(),
      model:S.device ? String(S.device.model||'').trim() : '', manufacturer:S.device ? String(S.device.manufacturer||'').trim() : '', bios_version:S.device ? String(S.device.bios_version||'').trim() : '',
      engineer:S.engineer ? S.engineer.name : '', ticket:String(m.ticket||'').trim() } }).then(function(path){
      m.idMsg = 'Сохранено: '+path; render(); A.mbLoadSaved();
      invoke('open_containing_folder', { path:path }).catch(function(){});
    }).catch(function(e){ m.idErr = typeof e==='string' ? e : 'Не удалось сохранить значения'; render(); });
  },
  mbUseSaved:function(i){
    var m = S.mb, x = (m.saved||[])[i]; if (!x) return;
    m.serial = x.serial || ''; m.uuid = x.uuid || ''; m.formErr = ''; m.idMsg = 'Подставлено из файла: '+(x.serial||'—')+(x.uuid?' · '+x.uuid:''); render();
  },
  /* Выбор файла вручную (системный диалог через <input type=file>): JSON из «Сохранить значения в файл» либо текст
     вида «UUID=…» / «SerialNumber=…» (как Info.txt из заводского ReadSN.cmd). */
  mbChooseFile:function(){ var el = document.getElementById('mb-file'); if (el) el.click(); },
  mbPickFile:function(inp){
    var f = inp && inp.files && inp.files[0]; if (!f) return;
    var m = S.mb; m.idErr = ''; m.idMsg = '';
    var r = new FileReader();
    r.onload = function(){ A.mbParseIdentity(String(r.result||''), f.name); inp.value = ''; };
    r.onerror = function(){ m.idErr = 'Не удалось прочитать файл'; render(); };
    r.readAsText(f);
  },
  mbParseIdentity:function(text, name){
    var m = S.mb, serial = '', uuid = '';
    text = text.replace(/^\uFEFF/, '');
    try {
      var j = JSON.parse(text);
      serial = j.serial || j.serial_number || j.SerialNumber || j.sn || '';
      uuid = j.uuid || j.UUID || '';
    } catch(e){
      text.split(/\r?\n/).forEach(function(l){
        var x = l.match(/^\s*(uuid|serialnumber|serial_number|serial|sn)\s*[=:]\s*(.+?)\s*$/i);
        if (!x) return;
        if (/^uuid$/i.test(x[1])) uuid = x[2]; else serial = x[2];
      });
    }
    serial = String(serial||'').trim(); uuid = String(uuid||'').trim().replace(/^\{|\}$/g, '');
    if (/^[0-9a-f]{32}$/i.test(uuid)) uuid = uuid.replace(/^(.{8})(.{4})(.{4})(.{4})(.{12})$/, '$1-$2-$3-$4-$5').toUpperCase();
    if (!serial && !uuid){ m.idErr = 'В файле «'+name+'» не найдены серийный номер и UUID'; render(); return; }
    var bad = [];
    if (serial && !isValidSerial(serial)) bad.push('серийный номер «'+serial+'» не подходит по формату');
    if (uuid && !isValidUuid(uuid)) bad.push('UUID «'+uuid+'» не в формате 8-4-4-4-12');
    if (bad.length){ m.idErr = 'Файл «'+name+'»: '+bad.join('; '); render(); return; }
    if (serial) m.serial = serial;
    if (uuid) m.uuid = uuid;
    m.formErr = ''; m.idMsg = 'Подставлено из файла «'+name+'»: '+(serial||'SN не указан')+(uuid?' · '+uuid:''); render();
  },
  mbOpenFolder:function(i){ var x = (S.mb.saved||[])[i]; if (x && x.path) invoke('open_containing_folder', { path:x.path }).catch(function(){}); },
  mbBack:function(){ S.mb.step='form'; render(); },
  mbWrite:function(){
    S.mb.step='writing'; render();
    invoke('write_smbios_identity', {
      technician: S.mb.techName,
      ticket: S.mb.ticket,
      beforeSerial: S.mb.before.serial_number,
      beforeUuid: S.mb.before.uuid,
      newSerial: S.mb.serial,
      newUuid: S.mb.uuid
    }).then(function(){
      S.mbLast = { serial:S.mb.serial, uuid:S.mb.uuid, at:Date.now() };
      invoke('mb_last_save', { serial:S.mb.serial||'', uuid:S.mb.uuid||'' }).catch(function(){});
      S.mb.step='done'; render();
    }).catch(function(err){
      // Запись не удалась (утилита не подтвердила, BIOS не поддерживается,
      // драйвер не загрузился и т. п.) — попытка уже в аудит-логе, показываем
      // причину как есть, а не притворяемся, что запись прошла.
      S.mb.writeError = typeof err==='string'?err:'Запись не выполнена';
      S.mb.step='stub';
      render();
    });
  },

  /* ---- отчёт ---- */
  /* Отчёт как объект — для экспорта (TXT/JSON/PDF) и для отправки админу. */
  buildReport:function(){
    var testable = CATS;
    return {
      device_model: deviceLabel(),
      device_serial: deviceKey(),
      intake: S.intake || '',
      repair_stage: S.repairStage || '',
      run_mode: S.auto && S.auto.on ? (S.autoMode==='express' ? 'экспресс' : 'полный') : (S.autoMode ? (S.autoMode==='express' ? 'экспресс' : 'полный') : ''),
      engineer: S.engineer ? S.engineer.name : '',
      summary_comment: S.reportSummary || '',
      // Часы ПК могли быть сбиты при запуске и потом синхронизированы (в отчёте была разница в 2 года) —
      // если «длительность» больше суток, начало считаем неточным и берём момент сборки отчёта
      started_at: (S.startedAt && Date.now()-new Date(S.startedAt).getTime() < 864e5 && Date.now() >= new Date(S.startedAt).getTime()) ? S.startedAt : new Date().toISOString(),
      finished_at: new Date().toISOString(),
      results: testable.map(function(c){
        var d = S.detail[c.id] || {}, a = d.auto || null;
        return { id:c.id, title:c.name, status: statusOf(c.id), comment: S.comments[c.id] || null,
          auto_status: a ? a.status : null, auto_note: a ? a.note : null,
          override_reason: d.override ? d.override.reason : null,
          details: (d.lines||[]).slice(0,200), in_profile: inProfile(c.id), finished_at: d.ts || null };
      })
    };
  },
  /* Отправка отчёта админу (Rust → приватный репозиторий отчётов, см.
     commands/upload.rs). Отчёт одного прогона лежит в одном и том же файле и
     обновляется при изменениях. Шлём только если содержимое изменилось с
     прошлой отправки (хэш без времени сборки) и в нём есть хоть один
     результат — поэтому экспорт без правок, повторные клики и периодическая
     проверка ничего не дублируют. kind: 'auto' (конец автопрогона), 'manual'
     (экспорт), 'sync' (отдельные тесты вне автопрогона, раз в ~20 с). */
  reportSync:function(kind){
    if (!S.engineer) return;
    if (S.reportBusy){ S.reportAgain = kind; return; }
    var rep = A.buildReport();
    if (!rep.results.some(function(r){ return r.status!=='idle'; })) return;
    var copy = JSON.parse(JSON.stringify(rep)); delete copy.finished_at;
    sha256Hex(JSON.stringify(copy)).then(function(h){
      if (h===S.sentHash) return;
      S.reportBusy = true;
      return invoke('submit_report', { kind:kind, report:rep }).then(function(r){
        S.sentHash = h; S.reportSend = /^sent/.test(r) ? 'sent' : 'queued';
      }).catch(function(){ S.reportSend = 'queued'; }).then(function(){
        S.reportBusy = false; refreshQueue();
        // пока инженер печатает в поле, экран не перерисовываем — иначе теряется фокус
        var ae = document.activeElement;
        if (ae && (ae.tagName==='TEXTAREA' || ae.tagName==='INPUT') && document.getElementById('screen').contains(ae)) renderNav(); else render();
        if (S.reportAgain){ var k = S.reportAgain; S.reportAgain = null; A.reportSync(k); }
      });
    });
  },
  exportReport:function(kind){
    var report = A.buildReport();
    A.reportSync('manual');
    var command = kind==='json' ? 'save_report_json' : kind==='pdf' ? 'save_report_pdf' : 'save_report_txt';
    invoke(command, { report: report }).then(function(path){
      S.exported = { kind: kind, path: path }; render();
      invoke('open_containing_folder', { path: path }).catch(function(){});
    }).catch(function(err){
      S.runError = typeof err==='string'?err:'Не удалось сохранить отчёт'; render();
    });
  },

  /* ---- вход по PIN (см. lockInit/renderLock ниже) ----
     Экран имени убран по запросу — только PIN; кто ввёл, определяется
     перебором _config/techs.json (приватный echips-reports) (совпадение хэша), имя показывается уже
     после успешного входа (сайдбар, отчёт), а не запрашивается заранее. */
  lockDigit:function(d){
    var L = S.lock;
    if(L.pin.length>=12) return; // генератор допускает PIN до 12 цифр
    L.err=''; L.pin += d; renderLock();
  },
  lockBackspace:function(){ S.lock.pin = S.lock.pin.slice(0,-1); S.lock.err=''; renderLock(); },
  lockSubmit:function(){ lockTrySubmit(); },
  lockRetry:function(){ lockInit(); },

  /* ---- добавление инженера (генератор записи для _config/techs.json (приватный echips-reports)) ----
     Доступно только администратору (S.engineer.role==='admin', см.
     renderNav/go) — сейчас это аккаунт Максима. */
  techadminField:function(k,v){ S.techadmin[k]=v; S.techadmin.err=''; },
  techadminBuild:function(){
    var t = S.techadmin;
    var id = (t.id||'').trim(), name = (t.name||'').trim(), pin = (t.pin||'').trim(), role = t.role==='admin' ? 'admin' : 'tech';
    if(!/^[a-z0-9_-]{2,32}$/i.test(id)){ t.err='Идентификатор: латиница/цифры/-/_, 2–32 символа.'; render(); return null; }
    if(!name){ t.err='Укажите ФИО.'; render(); return null; }
    if(!/^\d{4,12}$/.test(pin)){ t.err='PIN: только цифры, не меньше 4.'; render(); return null; }
    return { id:id, name:name, pin:pin, role:role };
  },
  /* Сохранение на сервере: PIN уходит по HTTPS и хэшируется там. Тот же id — заменяет запись (так меняется PIN/роль). */
  techadminPublish:function(){
    var t = S.techadmin;
    if(t.busy) return;
    var e = A.techadminBuild(); if(!e) return;
    t.busy=true; t.err=''; t.msg=''; render();
    invoke('techs_upsert', { id:e.id, name:e.name, pin:e.pin, role:e.role }).then(function(list){
      S.lock.techs = list;
      t.msg = 'Готово: «'+e.name+'» сохранён на сервере, вход с этим PIN работает сразу.';
      t.id=''; t.name=''; t.pin=''; t.role='tech'; t.result='';
    }).catch(function(err){ t.err = typeof err==='string' ? err : 'Не удалось сохранить'; })
      .then(function(){ t.busy=false; render(); });
  },
  techadminRemove:function(id){
    var t = S.techadmin;
    if(t.busy) return;
    if(S.engineer && S.engineer.id===id){ t.err='Себя удалить нельзя.'; render(); return; }
    if(!window.confirm('Удалить инженера «'+id+'» из списка?')) return;
    t.busy=true; t.err=''; t.msg=''; render();
    invoke('techs_remove', { id:id }).then(function(list){
      S.lock.techs = list; t.msg='Удалён: '+id;
    }).catch(function(err){ t.err = typeof err==='string' ? err : 'Не удалось удалить'; })
      .then(function(){ t.busy=false; render(); });
  },
  techadminInit:function(){
    invoke('techs_list').then(function(list){ S.lock.techs = list || []; render(); }).catch(function(err){ S.techadmin.err = typeof err==='string' ? err : 'Не удалось загрузить список'; render(); });
  },
  techadminCopy:function(){
    var text = S.techadmin.result;
    if(!text) return;
    (navigator.clipboard ? navigator.clipboard.writeText(text) : Promise.reject()).catch(function(){});
  },

  /* ---- админ-панель (Shift+F10, только role==='admin') ---- */
  adminClose:function(){ S.adminPanelOpen = false; renderAdminPanel(); }
};
window.echips = A;
S.startedAt = new Date().toISOString();

/* ---------- профиль модели ---------- */
function normCode(v){ return String(v||'').toUpperCase().replace(/[^A-Z0-9]/g,''); }
function profile(){
  var P = window.ECHIPS_PROFILES || { default:{ name:'Стандартный', tests:[], expect:{} }, models:{} };
  var base = P['default'], hay = S.device ? normCode(S.device.manufacturer+' '+S.device.model) : '', found = null;
  Object.keys(P.models||{}).forEach(function(k){
    if (!found && normCode(k) && hay.indexOf(normCode(k))>=0) found = P.models[k];
  });
  var out = {};
  Object.keys(base).forEach(function(k){ out[k] = base[k]; });
  if (found){
    Object.keys(found).forEach(function(k){ out[k] = found[k]; });
    out.expect = {};
    Object.keys(base.expect||{}).forEach(function(k){ out.expect[k] = base.expect[k]; });
    Object.keys(found.expect||{}).forEach(function(k){ out.expect[k] = found.expect[k]; });
  }
  return out;
}
function isRequired(id){
  var laptop = S.hw ? S.hw.is_laptop : true;
  return laptop && (profile().required||[]).indexOf(id)>=0;
}
/* Узел не найден: на ноутбуке из списка required — неисправность, иначе «не применимо». */
function absent(id, what){
  return isRequired(id)
    ? { status:'fail', note: what+': не обнаружено, для этой модели обязательно' }
    : { status:'na', note: what+': не обнаружено в системе' };
}

function sysReport(hw){
  var e = profile().expect || {}, lines = [], bad = [], warn = [];
  function chk(label, val, ok, exp){
    lines.push((ok===null ? '•' : ok ? '✓' : '✗') + ' ' + label + ': ' + val + (exp!=null && exp!=='' ? ' (ожидается ' + exp + ')' : ''));
    if (ok===false) bad.push(label);
  }
  function near(v, exp){ return exp>0 && Math.abs(v-exp)/exp <= 0.10; }
  chk('Процессор', hw.cpu.name+' · '+hw.cpu.cores+' ядер / '+hw.cpu.threads+' потоков'+(hw.cpu.max_mhz?' · '+hw.cpu.max_mhz+' МГц':''),
    e.cpu ? hw.cpu.name.toLowerCase().indexOf(String(e.cpu).toLowerCase())>=0 : null, e.cpu);
  chk('ОЗУ', hw.ram_total_gb.toFixed(1)+' ГБ, модулей: '+hw.ram_modules.length, e.ramGb ? near(hw.ram_total_gb, e.ramGb) : null, e.ramGb ? e.ramGb+' ГБ' : '');
  // На части плат WMI отдаёт одинаковое имя слота для разных модулей
  // (например, дважды "DIMM 0") — нумеруем повторы, чтобы не выглядело
  // как дубль одного и того же модуля.
  var slotSeen = {};
  hw.ram_modules.forEach(function(m){
    var n = (slotSeen[m.slot] = (slotSeen[m.slot]||0) + 1);
    var dupCount = hw.ram_modules.filter(function(x){ return x.slot===m.slot; }).length;
    var slotLabel = dupCount>1 ? 'Модуль '+n+' (BIOS называет слот «'+m.slot+'»)' : m.slot;
    lines.push('    '+slotLabel+': '+m.capacity_gb+' ГБ'+(m.speed_mhz?' · '+m.speed_mhz+' МГц':'')+(m.manufacturer?' · '+m.manufacturer:'')+(m.part_number?' · '+m.part_number:''));
  });
  var biggest = hw.disks.reduce(function(a,d){ return d.size_gb>(a?a.size_gb:0) ? d : a; }, null);
  chk('Диск', hw.disks.length ? hw.disks.map(function(d){ return d.model+' '+d.size_gb+' ГБ'+(d.media?' '+d.media:'')+(d.health&&d.health!=='Healthy'?' ['+d.health+']':''); }).join('; ') : 'не найден',
    e.diskGb ? (biggest ? near(biggest.size_gb, e.diskGb) : false) : (hw.disks.some(function(d){ return d.health && d.health!=='Healthy'; }) ? false : null), e.diskGb ? e.diskGb+' ГБ' : '');
  hw.gpus.forEach(function(g){ chk('Видео', g.name+(g.vram_mb?' · '+g.vram_mb+' МБ':'')+(g.driver_version?' · драйвер '+g.driver_version:''), null); });
  chk('Плата', hw.board || '—', null);
  chk('Серийник платы', hw.board_serial || '—', null);
  chk('UUID', hw.system_uuid || '—', null);
  // Предупреждения (вердикт не меняют): SMBIOS не прошит на заводе / расхождения модели
  var uu = String(hw.system_uuid||'').toLowerCase();
  if (!uu || /^0{8}-0{4}-0{4}-0{4}-0{12}$/.test(uu) || /^f{8}-f{4}-f{4}-f{4}-f{12}$/.test(uu) || uu==='03000200-0400-0500-0006-000700080009') warn.push('UUID не задан (заглушка производителя BIOS)');
  if (!hw.board_serial || /^(default string|to be filled|none|n\/a|0+|—)$/i.test(String(hw.board_serial).trim())) warn.push('серийник платы не задан');
  var boardClean = String(hw.board||'').split(/\s+/).filter(function(w){ return !isPlaceholder(w) && !/^default$/i.test(w) && !/^string$/i.test(w); }).join(' ');
  if (!boardClean) warn.push('плата в SMBIOS не заполнена (заглушка «'+String(hw.board||'—').trim()+'»)');
  else {
    var bd = boardClean.toLowerCase().replace(/[^a-z0-9]/g,''), md = S.device ? cleanSmbios(S.device.model).toLowerCase().replace(/[^a-z0-9]/g,'') : '';
    if (bd && md && bd.indexOf(md)<0 && md.indexOf(bd)<0) warn.push('модель устройства ('+S.device.model+') и плата ('+hw.board+') не совпадают');
  }
  warn.forEach(function(w){ lines.push('⚠ '+w); });
  var biosLow = String(hw.bios_version||'').toLowerCase(), biosList = (e.biosAny && e.biosAny.length) ? e.biosAny : (e.biosContains ? [e.biosContains] : []);
  chk('BIOS', (hw.bios_version||'—')+(hw.bios_date?' от '+hw.bios_date:''),
    biosList.length ? biosList.some(function(b){ return biosLow.indexOf(String(b).toLowerCase())>=0; }) : null, biosList.length ? biosList.join(' / ') : '');
  chk('Тип корпуса', hw.is_laptop ? 'ноутбук' : 'настольный ПК / другое', null);
  // справочно: TPM и Secure Boot вердикт не меняют (нет ≠ «не пройдено»)
  chk('TPM', hw.tpm || 'н/д', null);
  chk('Secure Boot', hw.secure_boot || 'н/д', null);
  return { lines:lines, bad:bad, warn:warn };
}

var ACT_ERRORS = {
  '0XC004F074':'не найден сервер KMS (корпоративная активация)', '0XC004F050':'ключ недействителен для этой редакции Windows',
  '0XC004C003':'ключ заблокирован или недействителен', '0XC004F034':'лицензия не найдена',
  '0X8007232B':'не найдено DNS-имя сервера KMS', '0X80072EE7':'нет доступа к интернету/DNS', '0XC004F009':'льготный период истёк',
  '0XC004F213':'в BIOS не найден ключ для этой машины (материнская плата заменена)', '0XC004C008':'ключ уже использован на другом количестве машин',
  '0XC004F014':'ключ не подходит для этой версии', '0XC004E016':'ключ не подходит для этой версии'
};
/* Статус PnP: неисправность — только Error/Degraded; Unknown бывает у составных
   устройств и корневых хабов без драйвера-фильтра и ошибкой не является. */
function isBadStatus(d){ return d.status==='Error' || d.status==='Degraded'; }
function statusRu(st){ return st==='OK' ? 'работает' : st==='Error' ? 'ОШИБКА' : st==='Degraded' ? 'работает с ограничениями' : st==='Unknown' ? 'состояние не сообщается' : st; }

/* ---------- реальные данные для runner-категорий ----------
   Каждая ветка возвращает { lines, verdict }: verdict — автооценка по порогам
   профиля ({status:'pass'|'fail'|'na', note}) или null, если оценить нельзя. */
function fetchCategory(kind){
  if (kind==='sys'){
    return invokeCached('get_hardware_summary', {}, 60000).then(function(hw){
      S.hw = hw;
      var r = sysReport(hw);
      var hasExp = Object.keys(profile().expect||{}).length>0;
      return { lines:r.lines, verdict: r.bad.length
        ? { status:'fail', note:'Не совпадает с профилем «'+profile().name+'»: '+r.bad.join(', ') }
        : { status:'pass', note: (hasExp ? 'Железо совпадает с профилем «'+profile().name+'»' : 'Сводка собрана (эталона в профиле нет)') + (r.warn.length ? ' · ⚠ '+r.warn.join('; ') : '') } };
    });
  }
  if (kind==='disk'){
    return invokeCached('get_disk_health', {}, 30000).then(function(list){
      var P = profile(), maxWear = P.diskMaxWearPct!=null ? P.diskMaxWearPct : 90, lines = [], bad = [];
      if (!list.length) return { lines:['Физические диски не найдены.'], verdict:{ status:'fail', note:'Диск не обнаружен' } };
      // В автопрогоне проверяем только системный диск (внешние/добавочные — вручную через «Категории»).
      var sysOnly = S.auto.on ? list.filter(function(d){ return d.is_system; }) : [];
      if (sysOnly.length){ lines.push('Автопрогон: проверяется только системный диск (других дисков в системе: '+(list.length-sysOnly.length)+')'); list = sysOnly; }
      list.forEach(function(d){
        lines.push(d.name+' · '+d.size_gb+' ГБ · '+(d.media||'тип не определён')+' · '+(d.bus||'')+(d.is_system?' · системный':''));
        lines.push('    Состояние: '+(d.health||'—')+(d.status?' ('+d.status+')':''));
        var extra = [];
        if (d.temp_c!=null) extra.push('температура '+d.temp_c+' °C');
        if (d.wear_pct!=null) extra.push('износ '+d.wear_pct+'%');
        if (d.power_on_hours!=null) extra.push('наработка '+d.power_on_hours+' ч');
        if (d.read_errors!=null) extra.push('ошибок чтения '+d.read_errors);
        if (d.write_errors!=null) extra.push('ошибок записи '+d.write_errors);
        lines.push('    '+(extra.length ? extra.join(' · ') : 'счётчики надёжности недоступны для этого диска'));
        if (d.health && d.health!=='Healthy') bad.push(d.name+': состояние '+d.health);
        if (d.wear_pct!=null && d.wear_pct>=maxWear) bad.push(d.name+': износ '+d.wear_pct+'% (порог '+maxWear+'%)');
        if ((d.read_errors||0)>0 || (d.write_errors||0)>0) bad.push(d.name+': неисправленные ошибки ввода-вывода');
        if (d.temp_c!=null && d.temp_c>=75) bad.push(d.name+': температура '+d.temp_c+' °C');
      });
      return { lines:lines, verdict: bad.length ? { status:'fail', note:bad.join('; ') } : { status:'pass', note:'Диски в порядке ('+list.length+' шт.)' } };
    });
  }
  if (kind==='ident'){
    return Promise.all([invoke('get_system_info'), invoke('get_hardware_summary'), invoke('list_lan_adapters').catch(function(){ return []; }), invoke('get_activation_status').catch(function(){ return null; }), invoke('mb_last_load').catch(function(){ return null; })]).then(function(r){
      var si = r[0]||{}, hw = r[1]||{}, lan = r[2]||[], act = r[3], last = r[4];
      // ожидаемые значения — последняя запись SN/UUID (хранится на диске и переживает перезагрузку)
      var exp = last ? { serial:last.serial||'', uuid:last.uuid||'' } : (S.mbLast || null);
      if (exp && !exp.serial && !exp.uuid) exp = null;
      // запись позже последней загрузки Windows → WMI ещё показывает старое: расхождение = «перезагрузите ПК», не ошибка
      var needReboot = !!(last && last.written_at && last.boot_time && new Date(last.written_at) > new Date(last.boot_time));
      var lines = [], bad = [], notes = [];
      function idBad(v){ v = String(v||'').trim(); return !v || isPlaceholder(v) || /^(system serial number|not applicable|0+|—)$/i.test(v); }
      function normU(u){ return String(u||'').toLowerCase().replace(/[^0-9a-f]/g,''); }
      function row(label, val, ok, extra){ lines.push((ok===null ? '•' : ok ? '✓' : '✗')+' '+label+': '+(val||'—')+(extra?' ('+extra+')':'')); if (ok===false) bad.push(label); }
      var sn = String(si.serial_number||'').trim(), bsn = String(hw.board_serial||'').trim(), uu = String(hw.system_uuid||'').trim();
      if (exp) lines.push('Сверка с последней записью в этой сессии ('+(exp.serial?'SN '+exp.serial:'')+(exp.serial&&exp.uuid?', ':'')+(exp.uuid?'UUID '+exp.uuid:'')+')');
      row('SN системы', sn, exp && exp.serial ? sn===exp.serial : !idBad(sn), exp && exp.serial ? 'ожидается '+exp.serial : idBad(sn) ? 'не задан' : '');
      // SN платы «To be filled by O.E.M.» — заводской заглушкой не считаем неисправностью (только показываем),
      // пока нет ожидаемого значения из записи SN/UUID; техник может засчитать проверку вручную
      var bsnPlaceholder = !(exp && exp.serial) && idBad(bsn);
      row('SN платы', bsn, exp && exp.serial ? bsn===exp.serial : bsnPlaceholder ? null : true, exp && exp.serial ? 'ожидается '+exp.serial : bsnPlaceholder ? 'не заполнено производителем' : '');
      if (bsnPlaceholder) notes.push('SN платы не заполнено производителем (заглушка BIOS) — на работу не влияет');
      if (!idBad(sn) && !idBad(bsn) && sn!==bsn) notes.push('SN системы и платы различаются');
      var uuBad = !uu || /^0{8}-0{4}-0{4}-0{4}-0{12}$/.test(uu) || /^f{8}-f{4}-f{4}-f{4}-f{12}$/i.test(uu) || uu.toLowerCase()==='03000200-0400-0500-0006-000700080009';
      row('UUID', uu, exp && exp.uuid ? normU(uu)===normU(exp.uuid) : !uuBad, exp && exp.uuid ? 'ожидается '+exp.uuid : uuBad ? 'заглушка BIOS' : '');
      lines.push(lan.length ? '• MAC (Ethernet): '+lan.map(function(a){ return (a.mac||'—')+' — '+(a.description||a.name||''); }).join('; ') : '• MAC (Ethernet): адаптера нет');
      lines.push('• Ключ Windows (OEM) в BIOS: '+(act ? (act.oem_key_present ? 'есть (…'+act.oem_key_tail+')' : 'нет') : 'не удалось прочитать'));
      notes.forEach(function(n){ lines.push('⚠ '+n); });
      if (exp && bad.length && needReboot) lines.push('ℹ Запись сделана после последней загрузки Windows — SMBIOS обновится после перезагрузки. Перезагрузите ПК и повторите проверку.');
      else if (exp && bad.length) lines.push('ℹ Запись была раньше последней загрузки Windows, а значения не совпадают — запись не применилась, повторите её.');
      return { lines:lines, verdict: bad.length && needReboot
        ? { status:'na', note:'Значения ещё не обновились (запись после последней загрузки Windows) — перезагрузите ПК и повторите проверку: '+bad.join(', ') }
        : bad.length
        ? { status:'fail', note:'Не совпадает или не задано: '+bad.join(', ') }
        : { status:'pass', note:'SN, плата и UUID в порядке'+(exp?' и совпадают с записанными':'')+(notes.length?' · ⚠ '+notes.join('; '):'') } };
    });
  }
  if (kind==='winact'){
    return invoke('get_activation_status').then(function(a){
      S.actRaw = a;
      var LS = { 0:'нет лицензии (не активирована)', 1:'лицензирована (активирована)', 2:'льготный период (OOB)', 3:'льготный период (OOT)', 4:'льготный период (не подлинная копия)', 5:'режим уведомлений (не активирована)', 6:'расширенный льготный период' };
      var lines = ['Windows: '+(a.os_caption||'—')+' · сборка '+(a.os_build||'—')];
      if (a.found){
        lines.push('Продукт: '+a.name);
        lines.push('Статус лицензии: '+(LS[a.license_status]!==undefined ? LS[a.license_status] : a.license_status));
        lines.push('Канал: '+(a.channel||'—')+' · ключ …'+a.partial_key);
        if (a.grace_minutes>0) lines.push('Осталось льготного периода: '+(a.grace_minutes/1440).toFixed(1)+' дн.');
        if (a.kms_machine) lines.push('Сервер KMS: '+a.kms_machine);
        if (a.reason && a.license_status!==1) lines.push('Код причины: '+a.reason+(ACT_ERRORS[a.reason.toUpperCase()] ? ' — '+ACT_ERRORS[a.reason.toUpperCase()] : ''));
      } else lines.push('Ключ продукта Windows не установлен.');
      lines.push('Ключ OEM в BIOS: '+(a.oem_key_present ? 'есть (…'+a.oem_key_tail+')' : 'нет'));
      var licensed = a.found && a.license_status===1, corp = /volume|kms|mak/i.test(a.channel||'') || !!a.kms_machine;
      var verdict = licensed
        ? { status:'pass', note:'Windows активирована ('+(a.channel||'канал не указан')+(corp?', корпоративная лицензия KMS/MAK — не OEM':'')+')' }
        : { status:'fail', note:'Windows не активирована: '+(a.found ? (LS[a.license_status]!==undefined ? LS[a.license_status] : 'статус '+a.license_status) : 'ключ не установлен')+(a.oem_key_present ? '. В BIOS есть OEM-ключ — можно установить и активировать' : '') };
      return { lines:lines, verdict:verdict };
    });
  }
  if (kind==='drv'){
    return invoke('list_problem_devices').then(function(all){
      // Служебные устройства, которые на ноутбуках показываются «без драйвера» без последствий
      // (PS/2-эмуляция мыши/клавиатуры при тачпаде на I2C/HID) — в замечания не берём, но пишем в лог.
      var BENIGN = /PS\/2[\s(]*(Mouse|Keyboard|мыш|клав)|PS\/2-совместим|Standard PS\/2/i;
      var list = all.filter(function(d){ return !BENIGN.test(d.friendly_name||''); });
      var skipped = all.filter(function(d){ return BENIGN.test(d.friendly_name||''); }).map(function(d){ return d.friendly_name+' (служебное, не считается)'; });
      var names = list.map(function(d){ return d.friendly_name + (d['class'] ? ' ('+d['class']+')' : ''); });
      return {
        lines: (list.length
          ? names.concat(['Установить драйверы можно во вкладке «Установка драйверов» — здесь только проверка, без установки.'])
          : ['Устройств без драйверов не найдено (Диспетчер устройств: ошибок нет).']).concat(skipped),
        verdict: list.length
          ? { status:'fail', note:'Без драйверов: '+list.length+' устройств — '+names.slice(0,3).join(', ')+(list.length>3?' и ещё '+(list.length-3):'')+'. Установить можно во вкладке «Установка драйверов»' }
          : { status:'pass', note:'Устройств без драйверов не найдено'+(skipped.length?' (служебных пропущено: '+skipped.length+')':'') }
      };
    });
  }
  if (kind==='crash'){
    var days = profile().crashDays || 30;
    return Promise.all([invokeAuto('get_crash_history', { days: Math.max(days, 90) }), invokeCached('get_disk_health', {}, 30000).catch(function(){ return []; })]).then(function(pair){
      var h = pair[0], disks = pair[1] || [];
      var laptop = !S.hw || S.hw.is_laptop;
      // \Device\HarddiskN\DRn -> имя диска с этим номером
      function diskName(text){
        return String(text).replace(/\\Device\\Harddisk(\d+)\\DR\d+/g, function(m, n){
          var d = disks.filter(function(x){ return String(x.number)===n; })[0];
          return 'Harddisk'+n+(d ? ' ('+d.name+')' : '');
        });
      }
      function wording(t){
        return laptop ? t : t.replace('Питание (батарея, зарядка/БП), перегрев или плата — либо удержание кнопки питания (в т. ч. при тестировании): Windows не различает эти случаи. Прогоните без батареи и с другим блоком питания, посмотрите температуры под нагрузкой.','Питание (блок питания, розетка/ИБП), перегрев или плата — либо удержание кнопки питания (в т. ч. при тестировании): Windows не различает эти случаи. Проверьте с другим блоком питания и другой розеткой, посмотрите температуры под нагрузкой.').replace('питание, зарядка/БП, перегрев, плата','питание (БП, розетка), перегрев, плата');
      }
      var SRC = { bugcheck:'событие WER', minidump:'minidump', 'kernel-power':'Kernel-Power' };
      var lines = [];
      var realCrashes = h.entries.filter(function(e){ return e.code!=='0x0'; }).length;
      // Отдельными строками "Метка: значение" (а не одной длинной через " · ") —
      // на экране отчёта/в PDF это распознаётся эвристикой как kv-сетка
      // (см. classify_detail в report.rs), как и одноимённые поля в
      // присланном пользователем PDF-шаблоне отчёта.
      lines.push('Сбоев: '+realCrashes);
      lines.push('Внезапных отключений: '+(h.entries.length-realCrashes));
      lines.push('Записей журнала до склейки: '+h.raw_records+' (сбой из нескольких источников считается один раз, окно 2 мин)');
      lines.push('Minidump-файлов: '+h.minidump_files);
      if (!h.entries.length) lines.push('Сбоев и внезапных перезагрузок в журнале не найдено.');
      h.entries.forEach(function(e){
        lines.push(e.time+' · '+e.code+' '+e.name+' · '+e.sources.map(function(x){ return SRC[x]||x; }).join(' + ')+(e.records>1?' (записей: '+e.records+')':''));
        lines.push('    '+wording(e.hint)+(e.params && e.params.length ? ' · параметры: '+e.params.join(', ') : ''));
      });
      if (h.hw_counts.whea || h.hw_counts.whea_corrected || h.hw_counts.disk || h.hw_counts.tdr){
        lines.push('WHEA критичные: '+h.hw_counts.whea);
        lines.push('WHEA исправленные: '+h.hw_counts.whea_corrected);
        lines.push('Ошибки диска: '+h.hw_counts.disk);
        lines.push('Сбросы видеодрайвера (TDR): '+h.hw_counts.tdr);
        h.hw_events.slice(0,8).forEach(function(e){ lines.push('    '+e.time+' · '+e.provider+' #'+e.id+' — '+diskName(e.text)); });
      }
      var actions = [];
      if (h.diagnosis.length){
        lines.push('— Диагноз по шаблону сбоев —');
        h.diagnosis.forEach(function(d){
          lines.push((d.level==='warn'?'⚠ ':'ℹ ')+d.title);
          lines.push('    '+wording(d.text));
          d.actions.forEach(function(x){ if (actions.indexOf(x)<0) actions.push(x); });
        });
      }
      var cutoff = Date.now() - days*86400000;
      var recent = h.entries.filter(function(e){ return new Date(e.time.replace(' ','T')).getTime() >= cutoff; });
      var crashes = recent.filter(function(e){ return e.code!=='0x0'; });
      var power = recent.filter(function(e){ return e.code==='0x0'; });
      var maxPower = profile().unexpectedShutdownsMax!=null ? profile().unexpectedShutdownsMax : 2;
      var v;
      if (crashes.length) v = { status:'fail', note:'Синих экранов за '+days+' дн.: '+crashes.length+' (последний '+crashes[0].code+' '+crashes[0].name+')'+(h.diagnosis.length && h.diagnosis[0].level==='warn' ? '. '+h.diagnosis[0].title : '') };
      else if (power.length > maxPower) v = { status:'fail', note:'Внезапных отключений/перезагрузок за '+days+' дн.: '+power.length+' (допустимо не более '+maxPower+') — питание, перегрев, плата' };
      else {
        var oldCrash = h.entries.filter(function(e){ return e.code!=='0x0'; }).length - crashes.length;
        var oldPower = h.entries.filter(function(e){ return e.code==='0x0'; }).length - power.length;
        var older = [];
        if (oldCrash>0) older.push('синих экранов '+oldCrash);
        if (oldPower>0) older.push('внезапных отключений '+oldPower);
        if (h.hw_counts.disk>0) older.push('ошибок диска '+h.hw_counts.disk);
        if (h.hw_counts.whea>0) older.push('критичных WHEA '+h.hw_counts.whea);
        v = { status:'pass', note:'Синих экранов за '+days+' дн. нет'+(power.length?'; внезапных отключений: '+power.length+' (в пределах допуска)':'')+(older.length ? '. Ранее (до 90 дн.): '+older.join(', ')+' — см. подробности' : '') };
      }
      return { lines:lines, verdict:v, actions:actions };
    });
  }
  if (kind==='usb'){
    return invoke('list_usb_devices').then(function(list){
      var bad = list.filter(isBadStatus);
      return {
        lines: list.length ? list.map(function(d){ return d.name + ' — ' + statusRu(d.status); }) : ['USB-устройства не обнаружены (кроме встроенных корневых хабов).'],
        verdict: bad.length ? { status:'fail', note:'USB-устройства с ошибкой: '+bad.slice(0,3).map(function(d){ return d.name; }).join(', ')+(bad.length>3?' и ещё '+(bad.length-3):'') }
                            : { status:'pass', note: list.length ? 'USB-устройства без ошибок ('+list.length+')' : 'Ошибок USB нет; порты проверьте флешкой' }
      };
    });
  }
  if (kind==='bt'){
    return invoke('list_bluetooth_devices').then(function(list){
      var bad = list.filter(isBadStatus);
      return {
        lines: list.length ? list.map(function(d){ return d.name + ' — ' + statusRu(d.status); }) : ['Bluetooth-адаптер не обнаружен или отключён.'],
        verdict: !list.length ? absent('bt','Bluetooth-адаптер')
          : bad.length ? { status:'fail', note:'Bluetooth с ошибкой: '+bad.map(function(d){ return d.name; }).join(', ') }
          : { status:'pass', note:'Bluetooth-адаптер работает' }
      };
    });
  }
  if (kind==='wifi'){
    var scan = invoke('scan_wifi_detailed').catch(function(){
      return invoke('scan_wifi_networks').then(function(l){ return l.map(function(x){ return { ssid:x, signal:null }; }); }).catch(function(){ return []; });
    });
    return Promise.all([invoke('list_wifi_adapters'), scan]).then(function(res){
      var adapters=res[0], networks=res[1], min = profile().wifiMinSignal;
      var lines = adapters.length
        ? adapters.map(function(a){ return 'Адаптер: ' + a.name + ' — ' + a.status + ' (' + a.mac + ')'; })
        : ['Wi-Fi адаптер не обнаружен.'];
      lines.push('Видимых сетей: ' + networks.length);
      var sigs = networks.map(function(n){ return n.signal; }).filter(function(v){ return v!=null; });
      var best = sigs.length ? Math.max.apply(null, sigs) : null;
      if (best!==null) lines.push('Лучший уровень сигнала: '+best+'%'+(min!=null?' (порог профиля '+min+'%)':''));
      var off = adapters.filter(function(a){ return a.status==='Disabled' || a.status==='Not Present'; });
      return { lines: lines.concat(networks.slice(0,8).map(function(n){ return n.ssid + (n.signal!=null ? ' — ' + n.signal + '%' : ''); })),
        verdict: !adapters.length ? absent('wifi','Wi-Fi адаптер')
          : off.length ? { status:'fail', note:'Wi-Fi адаптер отключён или недоступен' }
          : !networks.length ? { status:'fail', note:'Адаптер есть, но сетей не видит' }
          : (min!=null && best!==null && best<min) ? { status:'fail', note:'Лучший сигнал '+best+'% ниже порога '+min+'% — проверьте антенну и шлейф' }
          : { status:'pass', note:'Wi-Fi работает, видимых сетей: '+networks.length+(best!==null?', лучший сигнал '+best+'%':'') } };
    });
  }
  if (kind==='lan'){
    return invoke('list_lan_adapters').then(function(list){
      if (!list.length) return { lines:['Ethernet-адаптер не обнаружен.'], verdict:absent('lan','Ethernet-адаптер') };
      var up = list.filter(function(a){ return a.status==='Up'; });
      return { lines:list.map(function(a){ return a.name+' ('+a.description+') — '+(a.status==='Up'?'линк есть, '+a.speed:a.status==='Disconnected'?'кабель не подключён':a.status)+' · '+a.mac; }),
        verdict: up.length ? { status:'pass', note:'Линк установлен, скорость '+up[0].speed }
          : { status:'na', note:'Адаптер есть, кабель не подключён — линк не проверен' } };
    });
  }
  if (kind==='ext'){
    return invoke('list_monitors').then(function(list){
      var CONN = { HDMI:'HDMI', DVI:'DVI', DisplayPort:'DisplayPort', VGA:'VGA', internal:'встроенный', other:'тип не определён' };
      var ext = list.filter(function(m){ return !m.internal; });
      var lines = list.length ? list.map(function(m){ return (m.internal?'Встроенная матрица':'Внешний монитор')+': '+(m.name==='Monitor'?'Монитор':m.name)+' — '+(CONN[m.connection]||m.connection); }) : ['Мониторы через WMI не найдены.'];
      if (!ext.length) lines.push('Внешний монитор не подключён — подключите HDMI/DisplayPort и повторите проверку.');
      return { lines:lines, verdict: ext.length ? { status:'pass', note:'Внешний монитор подключён ('+(CONN[ext[0].connection]||ext[0].connection)+')' } : null };
    });
  }
  if (kind==='fp'){
    return invoke('get_fingerprint_sensor').then(function(name){
      return name
        ? { lines:['Сенсор обнаружен системой: ' + name, 'Пробную регистрацию и сравнение выполните вручную через Windows Hello.'],
            verdict:{ status:'pass', note:'Сенсор обнаружен ('+name+'); регистрацию пальца проверьте вручную' } }
        : { lines:['Сенсор отпечатка не обнаружен в системе (WinBio).'], verdict:absent('fp','Сенсор отпечатка') };
    });
  }
  if (kind==='bat'){
    return invoke('get_battery_info').then(function(b){
      if (!b.present) return { lines:['Батарея не обнаружена системой.'], verdict:absent('bat','Батарея') };
      var lines = ['Заряд: ' + b.charge_percent + '% (' + (b.charging?'заряжается':'от батареи') + ')'];
      var min = profile().batteryMinHealth, verdict = null;
      if (b.design_capacity_mwh!=null && b.full_charge_capacity_mwh!=null){
        lines.push('Design capacity: ' + b.design_capacity_mwh + ' мВт·ч');
        lines.push('Full charge capacity: ' + b.full_charge_capacity_mwh + ' мВт·ч');
        lines.push('Износ: ' + (100 - (b.health_percent||0)).toFixed(1) + '% (health ' + (b.health_percent||0).toFixed(1) + '%)');
        if (min!=null){
          lines.push('Порог профиля: health не ниже ' + min + '%');
          verdict = (b.health_percent||0) >= min
            ? { status:'pass', note:'Здоровье батареи '+(b.health_percent||0).toFixed(1)+'% (порог '+min+'%)' }
            : { status:'fail', note:'Здоровье батареи '+(b.health_percent||0).toFixed(1)+'% ниже порога '+min+'%' };
        }
      } else {
        lines.push('powercfg /batteryreport не вернул данные о ёмкости на этой машине.');
      }
      if (b.cycle_count!=null) lines.push('Циклов заряда: ' + b.cycle_count);
      if (b.charge_percent!=null && b.charge_percent<20){
        lines.push('⚠ Заряд ниже 20% — оценка ёмкости и поведения батареи при таком заряде неточна, повторите после подзарядки.');
        if (verdict) verdict = { status:verdict.status, note:verdict.note+' · ⚠ заряд '+b.charge_percent+'% (<20%)' };
      }
      return { lines:lines, verdict:verdict };
    });
  }
  return Promise.reject('Неизвестная категория');
}

/* ---------- сайдбар ---------- */
/* Состояние отправки отчётов в левом меню: последняя успешная отправка, очередь, причина неудачи */
function fmtClock(iso){ var d = new Date(iso); return isNaN(d) ? '' : String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0'); }
function renderQueue(){
  var el = document.getElementById('queue-text'), btn = document.getElementById('queue-send'); if (!el) return;
  var q = S.queue;
  if (!q){ el.textContent = ''; return; }
  var ok = q.last_ok ? '<span class="ok">Отправлено '+esc(fmtClock(q.last_ok))+'</span>' : 'Ещё не отправлялись';
  var wait = q.count>0 ? ' <span class="bad">В очереди: '+q.count+'</span>' : '';
  el.innerHTML = ok+wait;
  el.title = q.last_err ? String(q.last_err).slice(0,160) : '';
  if (btn) btn.style.display = (q.count>0 || q.last_err) ? '' : 'none';
}
function refreshQueue(){ invoke('report_queue_info').then(function(q){ S.queue = q; renderQueue(); }).catch(function(){}); }
function renderStageBtns(){
  ['before','after'].forEach(function(k){ var b=document.getElementById('stage-'+k); if (b) b.classList.toggle('on', S.repairStage===k); });
}
function renderNav(){
  renderStageBtns(); renderQueue();
  var active = { start:'start', drivers:'start', mb:'start', techadmin:'start', dash:'dash', test:'dash', sensors:'sensors', stress:'stress', report:'report', repdetail:'report', history:'history', events:'events' }[S.screen];
  var c = counts();
  var items = [
    { k:'start', label:'Режим', meta:'' },
    { k:'dash', label:'Категории', meta:c.checked+'/'+CATS.length },
    { k:'sensors', label:'Датчики', meta:S.sensorPoll?'live':'' },
    { k:'stress', label:'Стресс-тест', meta:S.st.running?'···':'' },
    { k:'report', label:'Отчёт', meta:'' }
  ];
  if (isAdmin()) items.push({ k:'history', label:'История', meta:'' });
  if (isAdmin()) items.push({ k:'events', label:'Журнал', meta:'' });
  document.getElementById('steps').innerHTML = items.map(function(i){
    return '<div class="step'+(i.k===active?' active':'')+'" onclick="echips.go(\''+i.k+'\')">'+
      '<span class="dot"></span><span class="lbl">'+i.label+'</span><span class="meta">'+i.meta+'</span></div>';
  }).join('');

  document.getElementById('devbox-name').textContent = S.device ? deviceLabel() : (S.deviceError ? 'ошибка определения' : 'определяется…');
  document.getElementById('devbox-sn').textContent = S.device ? ('SN ' + (S.device.serial_number || '—')) : '';
  var ii=document.getElementById('intake-input'); if (ii && document.activeElement!==ii && ii.value!==(S.intake||'')) ii.value=S.intake||'';
  document.getElementById('techbox-name').textContent = S.engineer ? S.engineer.name : '—';
  document.getElementById('techbox-add').style.display = isAdmin() ? '' : 'none';
  var av = document.getElementById('techbox-av'), rl = document.getElementById('techbox-role');
  if (av) av.textContent = S.engineer ? String(S.engineer.name||'?').charAt(0).toUpperCase() : '—';
  if (rl) rl.textContent = [isAdmin() ? 'админ' : '', S.engineer && S.engineer.offline ? 'без сети' : ''].filter(Boolean).join(' · ');
  var tb = document.getElementById('techbox'); if (tb) tb.title = S.engineer && S.engineer.offline ? 'Вход по сохранённой аренде без связи с сервером: отчёты отправятся, когда появится интернет' : '';
}

/* ---------- экраны ---------- */
function screenStart(){
  var d = S.device || {}, hw = S.hw || null;
  var mm = String(d.model||'').match(/\[([^\]]+)\]/), codeName = mm ? mm[1] : (cleanSmbios(d.model) || 'Ноутбук');
  var maker = cleanSmbios(d.manufacturer) + (mm ? ' · ' + String(d.model).replace(/\s*\[[^\]]+\]\s*/,'').trim() : '');
  var rows = [
    ['Процессор', hw && hw.cpu ? cpuShort(hw.cpu.name)+' · '+hw.cpu.cores+' ядер' : (d.cpu || '—')],
    ['Память', hw ? Math.round(hw.ram_total_gb)+' ГБ' : (d.ram_total_gb ? Math.round(d.ram_total_gb)+' ГБ' : '—')],
    ['Накопитель', hw && hw.disks && hw.disks.length ? hw.disks.filter(function(x){ return x.is_system; }).concat(hw.disks)[0].model+' · '+hw.disks.filter(function(x){ return x.is_system; }).concat(hw.disks)[0].size_gb+' ГБ' : '—'],
    ['BIOS', d.bios_version || '—'],
    ['Система', d.os_version || '—'],
    ['Серийный номер', d.serial_number || '—']
  ];
  var spec = rows.map(function(r){ return '<div><dt>'+r[0]+'</dt><dd>'+esc(r[1])+'</dd></div>'; }).join('');
  var status = S.device ? '<span class="pulse anim"></span>Устройство определено' : (S.deviceError ? 'Не удалось определить устройство: '+esc(S.deviceError) : '<span class="pulse"></span>Определяем устройство…');
  var tiles = [
    { t:'Экспресс', d:'Входной контроль, около 5 минут', act:"echips.autoStart('express')" },
    { t:'Драйверы', d:'Модель и пакеты с точкой восстановления', act:"echips.go('drivers')" },
    { t:'Диагностика', d:CATS.length+' проверок вручную', act:"echips.go('dash')" }
  ];
  if (FEATURE_MB) tiles.push({ t:'Замена платы', d:'Серийный номер и UUID', act:"echips.go('mb')" });
  return '<div class="pane">'+markerBanner()+
    '<div class="st-split"><section class="st-sheet"><p class="st-k">Подключено</p><h1 class="st-model">'+esc(codeName)+'</h1><p class="st-maker">'+esc(maker||'Устройство')+'</p>'+
      '<dl class="st-spec">'+spec+'</dl><div class="st-ok">'+status+'</div></section>'+
    '<section class="st-launch"><a class="st-hero" onclick="echips.autoStart()"><span class="st-hb"><b>Полный автопрогон</b><span>'+(profile().tests||[]).length+' проверок, отчёт уйдёт сам. Профиль: '+esc(profile().name)+'.</span></span><span class="st-go">Запустить</span></a>'+
      '<div class="st-tiles">'+tiles.map(function(x){ return '<a class="st-tile" onclick="'+x.act+'"><b>'+x.t+'</b><span>'+x.d+'</span></a>'; }).join('')+'</div></section></div></div>';
}

var DASH_AREAS = [
  ['Система', ['sys','ident','winact','drv']],
  ['Накопители', ['disk','crash','rem']],
  ['Сеть и порты', ['usb','bt','wifi','lan']],
  ['Ввод и экран', ['kb','lcd','ext','bright','cam','pad','fp','headset','touch','snd']],
  ['Питание, охлаждение, нагрузка', ['bat','fans','sens','mem','stress']]
];
function dashNote(id){
  var d = S.detail[id], a = d && d.auto;
  var t = (a && a.note) || S.comments[id] || '';
  return String(t).length>70 ? String(t).slice(0,68)+'…' : String(t);
}
function screenDash(){
  var c = counts(), na = 0, idle = 0, failed = [];
  CATS.forEach(function(x){ var st = statusOf(x.id); if (st==='na') na++; else if (st==='idle') idle++; else if (st==='fail') failed.push(x); });
  function tile(id){
    var x = CATS.filter(function(k){ return k.id===id; })[0]; if (!x) return '';
    if (x.group){
      var g = GROUPS[x.group], gs = groupStatus(x.group);
      return '<a class="dt '+STATUS[gs.st].cls+'" onclick="echips.openCat(\''+groupTests(x.group)[0].id+'\')"><span class="dn">'+g.name+'</span><span class="dr">'+gs.done+' из '+gs.total+' · '+STATUS[gs.st].label+'</span></a>';
    }
    var st = statusOf(x.id), live = x.kind==='sensors', out = st==='idle' && !inProfile(x.id);
    var note = dashNote(x.id) || (live ? 'мониторинг' : (out ? 'вне профиля' : STATUS[st].label));
    return '<a class="dt '+(live?'live ':'')+STATUS[st].cls+'" onclick="echips.openCat(\''+x.id+'\')"><span class="dn">'+x.name+'</span><span class="dr">'+esc(note)+'</span></a>';
  }
  var areas = DASH_AREAS.map(function(ar){
    return '<div class="da"><h3>'+ar[0]+'</h3><div class="dts">'+ar[1].map(tile).join('')+'</div></div>';
  }).join('');
  var attn = failed.length ? failed.map(function(x){
    var h = hintFor(x.id);
    return '<div class="dai"><div class="dah"><i class="ddot fail"></i><b>'+x.name+'</b></div><p>'+esc(dashNote(x.id)||'ошибка')+'</p>'+(h?'<p class="do">'+esc(h)+'</p>':'')+'</div>';
  }).join('') : '<div class="dai"><div class="dah"><i class="ddot pass"></i><b>Ошибок нет</b></div><p>Все выполненные проверки пройдены или не применимы.</p></div>';
  var waiting = CATS.filter(function(x){ return statusOf(x.id)==='idle' && inProfile(x.id); }).map(function(x){ return x.name; });
  return '<div class="pane">'+
    '<div class="dstats"><div class="dst pass"><b>'+c.pass+'</b><span>пройдено</span></div><div class="dst fail"><b>'+c.fail+'</b><span>ошибка</span></div><div class="dst na"><b>'+na+'</b><span>не применимо</span></div><div class="dst idle"><b>'+idle+'</b><span>ждут проверки</span></div>'+
    '<div class="headactions dacts"><button class="btn btn-ghost" onclick="echips.reset()">Сбросить</button><button class="btn btn-ghost" onclick="echips.autoStart(\'express\')">Экспресс</button><button class="btn btn-primary" onclick="echips.autoStart()">Автопрогон · полный</button><button class="btn btn-primary" onclick="echips.go(\'report\')">К отчёту</button></div></div>'+
    '<div class="dwrap"><section class="dmap">'+areas+'</section><aside class="dattn"><h3>Требует внимания</h3>'+attn+
      (waiting.length ? '<div class="dai"><div class="dah"><i class="ddot idle"></i><b>Не проверено</b></div><p>'+esc(waiting.slice(0,8).join(', '))+(waiting.length>8?' и ещё '+(waiting.length-8):'')+'.</p></div>' : '')+'</aside></div></div>';
}

/* Соответствие KeyboardEvent.code позициям клавиш в раскладке KEYROWS. */
/* Цифровой блок (id клавиш 'n:<code>') и мультимедиа-клавиши для Fn-комбинаций ('m:<code>').
   Самой клавиши Fn в KEYROWS сознательно нет: у большинства ноутбуков она обрабатывается
   прошивкой/EC и системе не видна вообще — никакое нажатие никогда не засчиталось бы, и
   техник каждый раз путал это с неисправностью. Проверяются её реальные комбинации
   (Fn+F-клавиши дают громкость, воспроизведение и т. п.) — см. MEDIA ниже. */
var NUMPAD = [
  ['NumLock','Num',1,1],['NumpadDivide','/',1,2],['NumpadMultiply','*',1,3],['NumpadSubtract','−',1,4],
  ['Numpad7','7',2,1],['Numpad8','8',2,2],['Numpad9','9',2,3],['NumpadAdd','+',2,4,2,1],
  ['Numpad4','4',3,1],['Numpad5','5',3,2],['Numpad6','6',3,3],
  ['Numpad1','1',4,1],['Numpad2','2',4,2],['Numpad3','3',4,3],['NumpadEnter','Enter',4,4,2,1],
  ['Numpad0','0',5,1,1,2],['NumpadDecimal','.',5,3]
];
var MEDIA = [['AudioVolumeUp','Гром. +'],['AudioVolumeDown','Гром. −'],['AudioVolumeMute','Mute'],['MediaPlayPause','Play/Pause'],['MediaTrackNext','След.'],['MediaTrackPrevious','Пред.']];
var CODEMAP = (function(){
  var named = { 'Esc':['Escape'],'Del':['Delete'],'Ins':['Insert'],'PrtScr':['PrintScreen'],'`':['Backquote'],'-':['Minus'],'=':['Equal'],'Bksp':['Backspace'],'Tab':['Tab'],
    '[':['BracketLeft'],']':['BracketRight'],'\\':['Backslash'],'Caps':['CapsLock'],';':['Semicolon'],"'":['Quote'],
    'Enter':['Enter'],',':['Comma'],'.':['Period'],'/':['Slash'],
    'Win':['MetaLeft','MetaRight'],'Space':['Space'],'←':['ArrowLeft'],'↑':['ArrowUp'],'↓':['ArrowDown'],'→':['ArrowRight'] };
  var map = {};
  KEYROWS.forEach(function(row,ri){
    row.forEach(function(label,ki){
      var id = ri+':'+ki, codes;
      // Индекс левой/правой клавиши в ряду определяем через indexOf, а не
      // жёстко зашитой колонкой — после удаления Fn из раскладки позиция
      // сдвинулась, а завязка на конкретный номер колонки была бы хрупкой.
      // Ctrl раньше был один блок на оба физических кода (ControlLeft и
      // ControlRight сразу) — техник не мог проверить правый Ctrl отдельно
      // ("нет в тесте правого Ctrl, при нажатии отображается левый"),
      // теперь в раскладке два блока Ctrl, разведены так же, как Shift/Alt.
      if (label==='Shift') codes = [ki===row.indexOf('Shift') ? 'ShiftLeft' : 'ShiftRight'];
      else if (label==='Alt') codes = [ki===row.indexOf('Alt') ? 'AltLeft' : 'AltRight'];
      else if (label==='Ctrl') codes = [ki===row.indexOf('Ctrl') ? 'ControlLeft' : 'ControlRight'];
      else if (named[label]) codes = named[label];
      else if (/^F\d+$/.test(label)) codes = [label];
      else if (/^\d$/.test(label)) codes = ['Digit'+label];
      else if (/^[A-Z]$/.test(label)) codes = ['Key'+label];
      else codes = [];
      codes.forEach(function(c){ map[c] = id; });
    });
  });
  NUMPAD.forEach(function(k){ map[k[0]] = 'n:'+k[0]; });
  MEDIA.forEach(function(k){ map[k[0]] = 'm:'+k[0]; });
  return map;
})();

/* Статистика по клавишам: число нажатий, автоповтор, дребезг (два нажатия быстрее 25 мс),
   удержание/залипание (нажата дольше 3 с). */
function kbStat(id){ return S.kstat[id] || (S.kstat[id] = { n:0, rep:0, chat:0, down:false, downAt:0, lastUp:0 }); }
function kbIssues(){
  var now = Date.now(), r = { chat:[], stuck:[], repeated:[] };
  Object.keys(S.kstat).forEach(function(id){
    var k = S.kstat[id];
    if (k.chat>0) r.chat.push(id);
    if (k.down && now-k.downAt>3000) r.stuck.push(id);
    if (k.n>1) r.repeated.push(id);
  });
  return r;
}
function kbLabel(id){
  if (id.indexOf('n:')===0){ var c=id.slice(2); var f=NUMPAD.filter(function(k){ return k[0]===c; })[0]; return 'Num '+(f?f[1]:c); }
  if (id.indexOf('m:')===0){ var c2=id.slice(2); var f2=MEDIA.filter(function(k){ return k[0]===c2; })[0]; return f2?f2[1]:c2; }
  var p = id.split(':'); return KEYROWS[p[0]] ? KEYROWS[p[0]][p[1]] : id;
}
/* Необязательные клавиши: Insert и медиа-ряд есть не на всех ноутбуках (часто это Fn-комбинации) —
   не считаются недобором в «нажато N из M», но остаются в списке проверки. */
function kbGroup(id){ return id.indexOf('n:')===0 ? 'numpad' : id.indexOf('m:')===0 ? 'media' : kbLabel(id)==='Ins' ? 'insert' : ''; }
/* Режим группы клавиш в профиле модели: profile.keyboard = { numpad, insert, media } = 'required' | 'optional' | 'absent'
   (по умолчанию: цифровой блок обязателен, Insert и медиа-ряд необязательны; 'absent' — клавиш нет, не показываются). */
function kbMode(id){
  var g = kbGroup(id); if (!g) return 'required';
  var m = (profile().keyboard || {})[g];
  return m || (g==='numpad' ? 'required' : 'optional');
}
function kbOptional(id){ return kbMode(id)==='optional'; }
function kbAbsent(id){ return kbMode(id)==='absent'; }
function kbAllIds(){
  var ids = [];
  KEYROWS.forEach(function(row,ri){ row.forEach(function(l,ki){ ids.push(ri+':'+ki); }); });
  NUMPAD.forEach(function(k){ ids.push('n:'+k[0]); });
  MEDIA.forEach(function(k){ ids.push('m:'+k[0]); });
  return ids.filter(function(i){ return !kbAbsent(i); });
}
function kbSummaryLines(){
  var ids = kbAllIds(), req = ids.filter(function(i){ return !kbOptional(i); }), opt = ids.filter(kbOptional);
  var pr = req.filter(function(i){ return S.keys[i]; }).length, po = opt.filter(function(i){ return S.keys[i]; }).length;
  var is = kbIssues(), lines = ['Нажато клавиш: '+pr+' из '+req.length+(opt.length ? ' (необязательные — Ins и медиа-ряд: '+po+' из '+opt.length+')' : '')];
  if (is.chat.length) lines.push('Дребезг (двойное срабатывание): '+is.chat.map(kbLabel).join(', '));
  if (is.stuck.length) lines.push('Залипание (нажата >3 с): '+is.stuck.map(kbLabel).join(', '));
  // у модификаторов автоповтор при удержании — норма, в замечания не выносим
  var many = Object.keys(S.kstat).filter(function(id){ return S.kstat[id].rep>0 && ['Shift','Ctrl','Alt','Win','Caps'].indexOf(kbLabel(id))<0; }).map(kbLabel);
  if (many.length) lines.push('Автоповтор при удержании: '+many.join(', '));
  if (S.lastUnknown) lines.push('Клавиша не в раскладке: '+S.lastUnknown);
  // какие именно клавиши не нажимались — иначе «96 из 102» ни о чём не говорит
  var miss = req.filter(function(i){ return !S.keys[i]; }).map(kbLabel), missOpt = opt.filter(function(i){ return !S.keys[i]; }).map(kbLabel);
  if (miss.length && miss.length<=40) lines.push('Не нажимались: '+miss.join(', '));
  if (missOpt.length) lines.push('Необязательные, не нажимались: '+missOpt.join(', '));
  return lines;
}
document.addEventListener('keyup', function(e){
  if (S.screen!=='test' || cat().kind!=='keyboard') return;
  // Клавишу отпустили раньше, чем сработал отложенный коммит ControlLeft
  // (см. ниже про AltGr) — например, короткий одиночный тап Ctrl без
  // Alt: пометим, чтобы коммит не выставил "нажата" уже после отпускания.
  if (e.code === 'ControlLeft' && S.altGrPendingCtrl) S.altGrPendingCtrl.released = true;
  var id = CODEMAP[e.code]; if (!id) return;
  var k = kbStat(id); k.down = false; k.lastUp = Date.now();
  render();
});
window.addEventListener('blur', function(){ Object.keys(S.kstat).forEach(function(id){ S.kstat[id].down = false; }); });
// Отмечает клавишу нажатой в статистике — вынесено из обработчика keydown
// отдельной функцией, т.к. для ControlLeft вызов теперь может отложиться
// (см. ниже про фантомный Ctrl от AltGr).
function kbCommitPress(id, now, stillDown){
  var k = kbStat(id);
  k.n++;
  if (k.n>1 && k.lastUp && now-k.lastUp<25) k.chat++;
  if (stillDown === false) { k.down = false; } else { k.down = true; k.downAt = now; }
  S.keys[id] = true;
  // пока есть нажатые клавиши, раз в 0,4 с обновляем признак залипания
  if (!S.kbT) S.kbT = setInterval(function(){
    var any = Object.keys(S.kstat).some(function(i){ return S.kstat[i].down; });
    if (S.screen==='test' && cat().kind==='keyboard' && any) render(); else if (!any){ clearInterval(S.kbT); S.kbT = null; }
  }, 400);
}
document.addEventListener('keydown', function(e){
  if (S.screen!=='test' || cat().kind!=='keyboard') return;
  if (e.target && (e.target.tagName==='INPUT' || e.target.tagName==='TEXTAREA')) return;
  e.preventDefault();
  var id = CODEMAP[e.code];
  if (!id){ S.lastUnknown = e.code; return; }
  var now = Date.now();

  // AltGr (правый Alt) на многих раскладках физически шлёт синтетическое
  // нажатие левого Ctrl прямо перед собой — так исторически работает
  // Windows для обратной совместимости (эмуляция Ctrl+Alt), не наш баг и
  // не дребезг реальной клавиши. Техник писал в отчёте: "при нажатии
  // правого alt считает нажатие на него и на левый ctrl". Ловим: не
  // засчитываем ControlLeft сразу, а откладываем на короткое окно — если
  // следом придёт AltRight, значит это и был фантомный Ctrl, гасим его и
  // считаем только настоящее нажатие Alt.
  if (e.code === 'ControlLeft' && !e.repeat) {
    S.altGrPendingCtrl = { id: id, at: now, released: false };
    setTimeout(function(){
      if (S.altGrPendingCtrl && S.altGrPendingCtrl.at === now) {
        kbCommitPress(id, now, !S.altGrPendingCtrl.released);
        S.altGrPendingCtrl = null;
        render();
      }
    }, 30);
    return;
  }
  if (e.code === 'AltRight' && S.altGrPendingCtrl && now - S.altGrPendingCtrl.at < 30) {
    S.altGrPendingCtrl = null; // фантомный Ctrl погашен, ниже считаем только AltRight
  }

  var k = kbStat(id);
  if (e.repeat){ k.rep++; return; }
  if (k.down && k.ext) return; // эту же клавишу уже засчитал опрос GetAsyncKeyState
  kbCommitPress(id, now);
  render();
});

// Клавиши, которые keyhook.rs глушит целиком для системы (Win-блокировка на
// время теста, см. kbWinToggle) — hook сам шлёт их обратно нам событием
// "hook-relay-key" (vk-код + нажата/отпущена), иначе тест бы вообще не
// увидел эти нажатия (низкоуровневый хук блокирует клавишу для всей
// системы, включая наш же webview). Числовые vk-коды — те же VK_*
// константы, что в keyhook.rs::is_blocked_vk (кроме Win — он не
// отображается в раскладке теста, ретранслировать нечего).
var HOOK_VK_CODE = {
  173:'AudioVolumeMute', 174:'AudioVolumeDown', 175:'AudioVolumeUp',
  176:'MediaTrackNext', 177:'MediaTrackPrevious', 178:'MediaStop', 179:'MediaPlayPause',
  44:'PrintScreen',
  // F5/F11/F12 — акселераторы самого WebView2 (Tauri на нём и работает):
  // F11 сворачивает наше окно в полноэкранный режим ("нажимаешь f11 —
  // увеличивается на весь экран" — с реального отчёта), F5 перезагрузил
  // бы страницу (потеряв весь ход теста), F12 открыл бы DevTools.
  // preventDefault() в JS их не останавливает — WebView2 перехватывает
  // раньше, чем событие доходит до DOM. 'F5'/'F11'/'F12' — уже готовые
  // id в CODEMAP (см. регэксп /^F\d+$/ там), отдельно заводить не нужно.
  116:'F5', 122:'F11', 123:'F12'
};
// Общий вход для клавиш, которые страница не видит сама (хук-ретрансляция и опрос GetAsyncKeyState):
// пока клавиша числится нажатой, повторные «нажатия» (автоповтор хука, второй источник) игнорируем.
function kbExternal(code, down){
  var id = CODEMAP[code];
  if (!id){ if (down){ S.lastUnknown = code; render(); } return; }
  var now = Date.now(), k = kbStat(id);
  if (down){ if (k.down) return; kbCommitPress(id, now); k.ext = true; }
  else { k.ext = false; if (!k.down) return; k.down = false; k.lastUp = now; }
  render();
}
tauriEvent.listen('hook-relay-key', function(ev){
  if (S.screen!=='test' || cat().kind!=='keyboard') return;
  var code = HOOK_VK_CODE[ev.payload.vk];
  if (code) kbExternal(code, !!ev.payload.down);
});
/* Опрос состояния клавиш, как в заводской утилите (GetAsyncKeyState): Win, PrtScr, медиа-ряд,
   F5/F11/F12 засчитываются без хука и блокировок. Обычные клавиши берём из DOM, чтобы не дублировать. */
var POLL_VK_CODE = { 91:'MetaLeft', 92:'MetaRight' };
Object.keys(HOOK_VK_CODE).forEach(function(v){ POLL_VK_CODE[v] = HOOK_VK_CODE[v]; });
var pollPrev = {}, pollBusy = false;
setInterval(function(){
  if (pollBusy || S.screen!=='test' || cat().kind!=='keyboard'){ if (S.screen!=='test' || cat().kind!=='keyboard') pollPrev = {}; return; }
  pollBusy = true;
  invoke('poll_pressed_keys').then(function(list){
    var cur = {};
    (list||[]).forEach(function(vk){ if (POLL_VK_CODE[vk]) cur[vk] = true; });
    Object.keys(cur).forEach(function(vk){ if (!pollPrev[vk]) kbExternal(POLL_VK_CODE[vk], true); });
    Object.keys(pollPrev).forEach(function(vk){ if (!cur[vk]) kbExternal(POLL_VK_CODE[vk], false); });
    pollPrev = cur;
  }).catch(function(){}).then(function(){ pollBusy = false; });
}, 40);

function keyCls(id){
  var k = S.kstat[id], now = Date.now(), c = 'key';
  if (S.keys[id]) c += ' on';
  if (k){
    if (k.down) c += (now-k.downAt>3000 ? ' stuck' : ' hold');
    if (k.chat>0) c += ' chat';
  }
  return c;
}
function keyBadge(id){ var k = S.kstat[id]; return k && k.n>1 ? '<b class="kcnt">×'+k.n+'</b>' : ''; }
function fieldKeyboard(){
  var allIds = kbAllIds(), reqIds = allIds.filter(function(i){ return !kbOptional(i); });
  var pressed = reqIds.filter(function(i){ return S.keys[i]; }).length, total = reqIds.length;
  var is = kbIssues();
  var main = '<div class="kbrows">'+ KEYROWS.map(function(row,ri){
      return '<div class="kbrow">'+ row.map(function(label,ki){
        var id = ri+':'+ki;
        if (kbAbsent(id)) return '';
        return '<div class="'+keyCls(id)+'" style="flex:'+(WIDE[label]||1)+' 1 0" onclick="echips.press(\''+id+'\')">'+esc(label)+keyBadge(id)+'</div>';
      }).join('') +'</div>';
    }).join('') +'</div>';
  var num = kbAbsent('n:NumLock') ? '' : '<div class="numpad">'+ NUMPAD.map(function(k){
      var id = 'n:'+k[0];
      return '<div class="'+keyCls(id)+'" style="grid-row:'+k[2]+(k[4]?' / span '+k[4]:'')+';grid-column:'+k[3]+(k[5]?' / span '+k[5]:'')+'" onclick="echips.press(\''+id+'\')">'+esc(k[1])+keyBadge(id)+'</div>';
    }).join('') +'</div>';
  var media = kbAbsent('m:AudioVolumeUp') ? '' : '<div class="kbmedia"><span class="kbnote">Fn-комбинации (мультимедиа):</span>'+ MEDIA.map(function(k){
      var id = 'm:'+k[0];
      return '<div class="'+keyCls(id)+' mk" onclick="echips.press(\''+id+'\')">'+esc(k[1])+keyBadge(id)+'</div>';
    }).join('') +'</div>';
  var st = [];
  if (is.chat.length) st.push('<span style="color:#F0C24B">дребезг: '+esc(is.chat.map(kbLabel).join(', '))+'</span>');
  if (is.stuck.length) st.push('<span style="color:var(--err)">залипание: '+esc(is.stuck.map(kbLabel).join(', '))+'</span>');
  var rep = Object.keys(S.kstat).filter(function(id){ return S.kstat[id].rep>0; }).length;
  return '<div class="kbwrap">'+
    '<div class="runrow" style="margin-bottom:10px">'+
    '<button class="btn '+(S.kbWinBlock?'btn-danger':'btn-ghost')+'" onclick="echips.kbWinToggle()">'+(S.kbWinBlock?'Разблокировать Win':'Заблокировать Win на время теста')+'</button>'+
    '<span class="n">'+(S.kbWinBlock?'Клавиша Win не открывает «Пуск», пока включено — не забудьте выключить после теста':'Win открывает меню «Пуск» и мешает проверке — включите блокировку на время теста')+'</span>'+
    (S.kbWinBlockErr ? '<span class="n" style="color:var(--err)">'+esc(S.kbWinBlockErr)+'</span>' : '')+
    '</div>'+
    '<div class="kbmeta"><span>RAW INPUT · нажмите каждую клавишу на ноутбуке</span>'+
    '<span>нажато '+pressed+' из '+total+' (обязательных) · rollover '+(pressed>3?'n-key ok':'—')+'</span></div>'+
    '<div class="kbboth">'+main+num+'</div>'+media+
    '<div class="kbnote">'+(st.length ? st.join(' · ')+' · ' : '')+'на клавише ×N — число нажатий (видно повторные нажатия); жёлтая рамка — дребезг (два срабатывания быстрее 25 мс); красная — клавиша нажата дольше 3 с (залипание). '+
    'Автоповтор при удержании: '+rep+' клавиш.'+(S.lastUnknown?' Не найдена в раскладке: '+esc(S.lastUnknown)+'.':'')+
    ' <button class="btn-link" onclick="echips.kbReset()">Сбросить счётчики</button></div></div>';
}
function paintFill(){
  var o = document.getElementById('fill-overlay'); if(!o) return;
  o.style.background = fillBg(FILLS[S.fill]);
  var h = document.getElementById('fill-hint');
  if (h){ h.style.opacity='1'; h.textContent = FILLS[S.fill].name+' · клик или → — следующий цвет · ← назад · Esc — выйти';
    clearTimeout(S.fillHintT); S.fillHintT = setTimeout(function(){ h.style.opacity='0'; }, 3500); }
}
document.addEventListener('keydown', function(e){
  if (!document.getElementById('fill-overlay')) return;
  e.preventDefault();
  if (e.key==='Escape') A.fillClose();
  else if (e.key==='ArrowRight' || e.key===' ' || e.key==='Enter') A.nextFill();
  else if (e.key==='ArrowLeft') A.prevFill();
});
function fieldDisplay(){
  return '<div class="fillwrap"><div class="btn-row-fs"><button class="btn btn-primary" onclick="echips.fillOpen()">На весь экран</button>'+
    '<span class="kbnote">Полноэкранная заливка: клик или → — следующий цвет, Esc — выход</span></div>'+
    '<div class="fillstage" style="background:'+fillBg(FILLS[S.fill])+'" onclick="echips.nextFill()">'+
    '<span>клик — следующая заливка · '+FILLS[S.fill].name+'</span></div>'+
    '<div class="swatches">'+ FILLS.map(function(f,i){
      return '<div class="swatch'+(i===S.fill?' on':'')+'" style="background:'+fillBg(f)+'" onclick="echips.setFill('+i+')"></div>';
    }).join('') +'</div></div>';
}
function fieldTouchpad(){
  return '<div class="padwrap">'+
    '<div class="pad" id="pad">'+ (S.padDots.length?'':'<div class="ph">проведите по полю — точки касания и жесты</div>') +
      S.padDots.map(function(d){ return '<div class="tp" style="left:'+d.x+'%;top:'+d.y+'%"></div>'; }).join('') +
    '</div>'+
    '<div class="side">'+
      stat('касаний за сессию', S.padCount) + stat('макс. одновременно', S.padMax) + stat('событий move', S.padMoves) +
    '</div></div>';
}
function stat(k,v){ return '<div class="stat"><div class="k">'+k+'</div><div class="v">'+v+'</div></div>'; }
function fieldRunner(){
  var c = cat();
  var body = S.runError ? '<div class="idle" style="color:var(--err)"><span class="t">--</span><span>'+esc(S.runError)+'</span></div>'
    : S.runLines.length ? S.runLines.map(function(t,i){
        return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span>'+esc(t)+'</span></div>';
      }).join('')
    : '<div class="idle"><span class="t">--</span><span>нажмите «Запустить проверку»</span></div>';
  if(S.running) body += '<div class="wait"><span class="t">··</span><span>выполняется…</span></div>';
  var note = S.running ? 'опрашиваем систему' : S.runLines.length ? 'лог ниже попадёт в отчёт' : 'реальный запрос к системе (WMI)';
  return '<div class="runwrap"><div class="runrow">'+
    '<button class="btn btn-primary" onclick="echips.run()" '+(S.running?'disabled':'')+'>'+(S.running?'Идёт проверка…':S.runLines.length?'Повторить':'Запустить проверку')+'</button>'+
    '<span class="n">'+note+'</span></div>'+
    '<div class="log">'+body+'</div>'+
    (S.verdict && S.verdict.status ? '<div class="kbnote" style="margin-top:8px">Автооценка: '+({pass:'пройден',fail:'не пройден',na:'не применимо'}[S.verdict.status])+' — '+esc(S.verdict.note)+'</div>' : '')+
    (c.fetch==='winact' ? winactPanel() : '')+
    (c.fetch==='bat' ? batLivePanel() : '')+
    (c.fetch==='sys' ? sysProfilePanel() : '')+
    (S.runActions.length && !S.auto.on ? '<div class="recbox"><span class="kbnote">Рекомендуемые проверки по итогам анализа:</span>'+ S.runActions.map(function(id){
        var t = CATS.filter(function(x){ return x.id===id; })[0]; if (!t) return '';
        return '<button class="btn btn-ghost" onclick="echips.openCat(\''+id+'\')">'+esc(t.name)+'</button>';
      }).join('')+'</div>' : '')+'</div>';
}
/* Режим «в реальном времени» для аккумулятора (вне автопрогона): раз в 2 с читаем
   напряжение, мощность заряда/разряда и процент. Автопрогон по-прежнему проверяет только здоровье. */
function batLiveHtml(){
  var d = S.batLive.data;
  if (!d) return '<div class="idle"><span class="t">--</span><span>'+(S.batLive.on?'опрос…':'нажмите «Смотреть в реальном времени»')+'</span></div>';
  if (!d.present) return '<div class="idle"><span class="t">--</span><span>Батарея не обнаружена</span></div>';
  function v(x, f){ return x==null ? '—' : f(x); }
  var chg = d.charge_rate_mw>0, dis = d.discharge_rate_mw>0;
  var rate = chg ? '+'+(d.charge_rate_mw/1000).toFixed(1)+' Вт (заряд)' : dis ? '−'+(d.discharge_rate_mw/1000).toFixed(1)+' Вт (разряд)' : (d.charge_rate_mw==null ? '—' : '0 Вт');
  return '<div class="stats4">'+
    '<div class="stat4"><div class="k">заряд</div><div class="v">'+v(d.charge_percent,function(x){return x+'%';})+'</div></div>'+
    '<div class="stat4"><div class="k">напряжение</div><div class="v">'+v(d.voltage_mv,function(x){return (x/1000).toFixed(2)+' В';})+'</div></div>'+
    '<div class="stat4"><div class="k">мощность</div><div class="v">'+rate+'</div></div>'+
    '<div class="stat4"><div class="k">остаток</div><div class="v">'+v(d.remaining_mwh,function(x){return (x/1000).toFixed(1)+' Вт·ч';})+'</div></div></div>'+
    '<div class="kbnote" style="margin-top:8px">'+(d.power_online==null?'':(d.power_online?'Питание от сети. ':'Работа от батареи. '))+(d.voltage_mv==null?'Этот ноутбук не отдаёт напряжение и мощность через WMI.':'')+'</div>';
}
function batLivePanel(){
  var B = S.batLive;
  return '<div class="actpanel"><div class="runrow"><button class="btn btn-ghost" onclick="echips.batLiveToggle()">'+(B.on?'Остановить наблюдение':'Смотреть в реальном времени')+'</button>'+
    '<span class="n">обновление раз в 2 с; в отчёт не попадает</span></div><div id="bat-live" style="margin-top:10px">'+batLiveHtml()+'</div></div>';
}
/* Эталон модели (только админ): снимает данные текущего ноутбука и сохраняет профиль в git
   (data/profiles.json) — следующие ноутбуки этой модели сверяются с ним без пересборки exe. */
function cpuShort(name){
  var n = String(name||'');
  var m = n.match(/i[3579]-\w+/i) || n.match(/Ryzen\s+\d\s+\w+/i) || n.match(/Core\s+Ultra\s+\d\s+\w+/i) || n.match(/\b[NJ]\d{2,4}\b/) || n.match(/\b\d{4}[A-Z]?\b/);
  return m ? m[0] : n.replace(/\(R\)|\(TM\)|CPU|@.*$/gi, '').replace(/\s+/g,' ').trim();
}
function profileFromHw(hw){
  var biggest = hw.disks.reduce(function(a,d){ return d.size_gb>(a?a.size_gb:0) ? d : a; }, null);
  var sys = hw.disks.filter(function(d){ return d.is_system; })[0];
  return { cpu: cpuShort(hw.cpu.name), ramGb: Math.round(hw.ram_total_gb), diskGb: Math.round((sys||biggest||{size_gb:0}).size_gb) };
}
function sysProfilePanel(){
  if (!isAdmin() || !S.hw) return '';
  var P = S.profSave || (S.profSave = { key:'', msg:'', err:'', busy:false, maxT:'', bios:false });
  if (!P.key && S.device) P.key = cleanSmbios(S.device.model) || '';
  var e = profileFromHw(S.hw), known = profile().name!=='Стандартный';
  return '<div class="actpanel"><div class="k">Эталон модели (админ)</div>'+
    '<div class="kbnote" style="margin:6px 0">'+(known ? 'Для этой модели профиль уже есть: «'+esc(profile().name)+'». Сохранение заменит его. ' : 'Профиля для этой модели нет. ')+
    'Будет записано: процессор «'+esc(e.cpu)+'», ОЗУ '+e.ramGb+' ГБ, системный диск '+e.diskGb+' ГБ.</div>'+
    '<div class="runrow" style="margin-bottom:8px"><label class="kbnote"><input type="checkbox" '+(P.bios?'checked ':'')+'onclick="echips.profBios(this.checked)"> добавить текущую версию BIOS ('+esc(S.hw.bios_version||'—')+') в допустимые</label>'+
    '<input class="search-input" style="max-width:190px" type="number" min="60" max="110" value="'+esc(P.maxT)+'" placeholder="порог темп., °C (сейчас '+(profile().maxTempC||95)+')" oninput="echips.profMaxT(this.value)"></div>'+
    '<div class="runrow"><input class="search-input" style="max-width:260px" value="'+esc(P.key)+'" placeholder="ключ модели, например NB101A" oninput="echips.profKey(this.value)">'+
    '<button class="btn btn-ghost" onclick="echips.profSave()" '+(P.busy?'disabled':'')+'>'+(P.busy?'Сохранение…':'Сохранить эталон в git')+'</button></div>'+
    (P.msg?'<div class="kbnote" style="margin-top:8px;color:var(--ok)">'+esc(P.msg)+'</div>':'')+
    (P.err?'<div class="kbnote" style="margin-top:8px;color:var(--err)">'+esc(P.err)+'</div>':'')+'</div>';
}
function winactPanel(){
  var a = S.actRaw, act = S.act;
  if (!a) return '';
  var licensed = a.found && a.license_status===1;
  var LABELS = {
    activate:'Запустить онлайн-активацию Windows (нужен интернет)?',
    install_oem_key:'Установить OEM-ключ, вшитый в BIOS, и активировать Windows?',
    restart_service:'Перезапустить службу лицензирования (sppsvc)?',
    sync_time:'Синхронизировать системное время? (неверная дата мешает активации)',
    install_key:'Установить введённый ключ и активировать Windows?'
  };
  var out = '<div class="actpanel"><div class="kbnote" style="margin-bottom:8px">'+(licensed
    ? 'Windows активирована — устранение не требуется.'
    : act.autoFixRunning ? 'Активация не пройдена — приложение само пробует штатные шаги устранения (см. лог выше)…'
    : 'Устранение проблем с активацией. Только штатные способы Windows: онлайн-активация, OEM-ключ из BIOS и ваш собственный ключ.')+'</div>';
  if (act.autoFixRunning){
    // Автоустранение уже идёт (см. run()/winactAutoFix) — не даём запустить второй раз теми же кнопками.
  } else if (act.confirm){
    out += '<div class="actconfirm">'+esc(LABELS[act.confirm]||'Выполнить?')+
      '<div class="headactions" style="margin-top:10px"><button class="btn btn-ghost" onclick="echips.actCancel()" '+(act.busy?'disabled':'')+'>Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.actRun()" '+(act.busy?'disabled':'')+'>'+(act.busy?'Выполняется…':'Да, выполнить')+'</button></div></div>';
  } else if (!licensed){
    out += '<div class="runrow" style="flex-wrap:wrap">'+
      '<button class="btn btn-primary" onclick="echips.actAsk(\'activate\')">Онлайн-активация</button>'+
      (a.oem_key_present ? '<button class="btn btn-ghost" onclick="echips.actAsk(\'install_oem_key\')">Ключ из BIOS (…'+esc(a.oem_key_tail)+')</button>' : '')+
      '<button class="btn btn-ghost" onclick="echips.actAsk(\'restart_service\')">Перезапустить службу лицензирования</button>'+
      '<button class="btn btn-ghost" onclick="echips.actAsk(\'sync_time\')">Синхронизировать время</button>'+
      '<button class="btn btn-ghost" onclick="echips.actOpen()">Параметры активации Windows</button>'+
      '<button class="btn btn-ghost" onclick="echips.actKeyToggle()">'+(act.keyOpen?'Скрыть ввод ключа':'Ввести ключ вручную')+'</button></div>';
    if (act.keyOpen){
      out += '<div class="runrow" style="margin-top:10px"><input class="keyinput" placeholder="XXXXX-XXXXX-XXXXX-XXXXX-XXXXX" maxlength="29" value="'+esc(act.key)+'" oninput="echips.actKey(this.value)">'+
        '<button class="btn btn-primary" onclick="echips.actAsk(\'install_key\')">Установить ключ</button></div>';
    }
  }
  if (act.msg) out += '<div class="kbnote" style="margin-top:8px;color:var(--ok)">'+esc(act.msg)+'</div>';
  if (act.err) out += '<div class="kbnote" style="margin-top:8px;color:var(--err)">'+esc(act.err)+'</div>';
  return out+'</div>';
}
function fieldCamera(){
  return '<div class="camwrap"><div class="preview" style="position:relative;overflow:hidden">'+
    (S.camStream
      ? '<video id="cam-preview" autoplay muted playsinline style="width:100%;height:100%;object-fit:cover;border-radius:10px"></video>'
      : '<div class="lens">CAM</div><div class="m">'+(S.runError?esc(S.runError):'запрос доступа к камере…')+'</div>')+
    '</div>'+
    '<div class="side">'+ CAMCHECKS.map(function(s){ return '<div class="note">'+s+'</div>'; }).join('') +
    (S.camStream ? '<button class="btn btn-ghost" style="margin-top:8px" onclick="echips.camRecord()" '+(S.camRecording?'disabled':'')+'>'+(S.camRecording?'Запись 5 с…':S.camClip?'Записать заново':'Записать 5 с')+'</button>' : '')+
    (S.camClip ? '<video src="'+S.camClip+'" controls style="width:100%;margin-top:8px;border-radius:8px;background:#000"></video>' : '')+'</div></div>';
}
function spectrumBars(){
  var bars = '';
  var data = null;
  if (S.toneAnalyser){
    data = new Uint8Array(S.toneAnalyser.frequencyBinCount);
    S.toneAnalyser.getByteFrequencyData(data);
  }
  for(var i=0;i<36;i++){
    var h = 4;
    if (data){
      var idx = Math.floor(i/36*data.length);
      h = 4 + (data[idx]/255)*90;
    }
    bars += '<i style="height:'+h.toFixed(0)+'%"></i>';
  }
  return bars;
}
function fieldAudio(){
  var bars = spectrumBars();
  var note = S.tone===null ? 'выберите сигнал — спектр появится ниже'
    : S.tone===2 ? 'echo-тест: сигнал с микрофона идёт в анализатор напрямую (Web Audio)'
    : 'воспроизведение через встроенные динамики · Web Audio API';
  return '<div class="audiowrap"><div class="tones">'+ TONES.map(function(t,i){
      return '<button class="tone'+(S.tone===i?' on':'')+'" onclick="echips.tone('+i+')">'+t+'</button>';
    }).join('') +'</div>'+
    '<div class="spectrum">'+bars+'</div><div class="kbnote">'+note+'</div></div>';
}

function fieldHeadset(){
  var d = S.inp, bars = spectrumBars();
  var st = !d ? 'определяется…' : d.headset ? 'Наушники подключены: '+esc(d.headset_name||'гнездо 3,5 мм') : 'Наушники не обнаружены — подключите их к гнезду 3,5 мм (в автопрогоне тест будет «не применим»)';
  var note = S.tone===null ? 'выберите сигнал: звук должен идти в наушники, левый и правый канал по очереди'
    : S.tone===1 ? 'левый / правый канал: убедитесь, что слышны оба' : 'тон 1 кГц в наушники';
  return '<div class="audiowrap"><div class="kbnote" style="margin-bottom:10px">'+st+'</div><div class="tones">'+
    [0,1].map(function(i){ return '<button class="tone'+(S.tone===i?' on':'')+'" onclick="echips.tone('+i+')">'+TONES[i]+'</button>'; }).join('')+'</div>'+
    '<div class="spectrum">'+bars+'</div><div class="kbnote">'+note+'</div></div>';
}
function fieldTouch(){
  var d = S.inp, t = S.tch || { dots:[], count:0, max:0, moves:0 };
  var st = !d ? 'определяется…' : d.touch ? 'Сенсорный экран обнаружен — коснитесь поля пальцами (в т. ч. несколькими сразу)' : 'Сенсорный экран не обнаружен (в автопрогоне тест «не применим»)';
  return '<div class="kbnote" style="margin-bottom:8px">'+st+' · мышь и тачпад не считаются</div>'+
    '<div class="padwrap"><div class="pad" id="touch-area">'+(t.dots.length?'':'<div class="ph">коснитесь экрана — точки касания и мультитач</div>')+
    t.dots.map(function(x){ return '<div class="tp" style="left:'+x.x+'%;top:'+x.y+'%"></div>'; }).join('')+'</div>'+
    '<div class="side"><div class="stat"><div class="k">касаний</div><div class="v" id="tch-count">'+t.count+'</div></div>'+
    '<div class="stat"><div class="k">макс. одновременно</div><div class="v" id="tch-max">'+t.max+'</div></div>'+
    '<div class="stat"><div class="k">событий move</div><div class="v" id="tch-moves">'+t.moves+'</div></div></div></div>';
}
/* Касания сенсорного экрана: DOM обновляем напрямую (полная перерисовка посреди касания рвёт pointer-события). */
function bindTouchArea(){
  var ta = document.getElementById('touch-area'); if (!ta || ta._bound) return; ta._bound = true;
  function upd(){ var t=S.tch; ['count','max','moves'].forEach(function(k){ var el=document.getElementById('tch-'+k); if (el) el.textContent = t[k]; }); }
  function dot(e){
    var r = ta.getBoundingClientRect(), t = S.tch, x = ((e.clientX-r.left)/r.width*100).toFixed(1), y = ((e.clientY-r.top)/r.height*100).toFixed(1);
    t.dots.push({ x:x, y:y }); if (t.dots.length>140) t.dots.shift();
    var ph = ta.querySelector('.ph'); if (ph) ph.remove();
    var d = document.createElement('div'); d.className = 'tp'; d.style.left = x+'%'; d.style.top = y+'%'; ta.appendChild(d);
    while (ta.querySelectorAll('.tp').length>140) ta.querySelector('.tp').remove();
  }
  ta.addEventListener('pointerdown', function(e){
    if (e.pointerType==='mouse') return;
    var t = S.tch; t.active[e.pointerId] = true; t.count++; t.touched = true;
    t.max = Math.max(t.max, Object.keys(t.active).length); dot(e); upd();
  });
  ta.addEventListener('pointermove', function(e){ if (e.pointerType!=='mouse' && S.tch.active[e.pointerId]){ S.tch.moves++; dot(e); upd(); } });
  ['pointerup','pointercancel','pointerleave'].forEach(function(n){ ta.addEventListener(n, function(e){ delete S.tch.active[e.pointerId]; }); });
}

/* ---------- SMART ---------- */
var SMART_LABEL = { good:'Хорошее', caution:'Тревога', bad:'Плохое', unknown:'Нет данных' };
function smartReasons(d){
  var out = [];
  if (d.kind==='ata'){
    d.attrs.forEach(function(a){
      if (a.status==='bad') out.push(a.name+' ниже порога ('+a.current+' ≤ '+a.threshold+')');
      else if (a.status==='warn') out.push(a.name+' = '+(a.raw % 4294967296));
    });
  } else if (d.kind==='nvme' && d.nvme){
    var h = d.nvme;
    if (h.critical_warning) out.push('критическое предупреждение 0x'+h.critical_warning.toString(16).toUpperCase());
    if (h.available_spare<=h.spare_threshold && h.spare_threshold>0) out.push('резерв '+h.available_spare+'% ≤ порога '+h.spare_threshold+'%');
    if (h.percentage_used>=50) out.push('износ '+h.percentage_used+'%');
    if (h.media_errors>0) out.push('ошибок целостности данных: '+h.media_errors);
  }
  if (d.health_pct!=null && d.health_pct<=50 && d.kind!=='nvme') out.push('остаток ресурса '+d.health_pct+'%');
  d.notes.forEach(function(n){ if (/предсказывает/.test(n)) out.push(n); });
  return out;
}
function judgeSmart(list){
  var P = profile(), cautionFail = P.smartCautionIsFail!==false, bad=[], caution=[], have=0;
  list.forEach(function(d){
    if (d.status!=='unknown') have++;
    if (d.status==='bad') bad.push(d.name+': '+(smartReasons(d).join(', ')||'плохое состояние'));
    else if (d.status==='caution') caution.push(d.name+': '+(smartReasons(d).join(', ')||'требует внимания'));
  });
  if (bad.length) return { status:'fail', note:'SMART — плохое состояние. '+bad.join('; ') };
  if (caution.length && cautionFail) return { status:'fail', note:'SMART — тревога. '+caution.join('; ') };
  if (!have) return null;
  // ошибки обмена по SATA (Phy/CRC) на здоровье не влияют, но при росте — тревожный признак: пишем предупреждение в итог
  var ifaceWarn = [];
  list.forEach(function(d){ (d.attrs||[]).forEach(function(a){ if (/Phy Error|CRC Error/i.test(a.name) && a.raw>=100 && a.raw<=9007199254740991) ifaceWarn.push(d.name+': '+a.name+' = '+a.raw); }); });
  return { status:'pass', note:(ifaceWarn.length ? '⚠ Предупреждение: ошибки обмена по SATA-интерфейсу ('+ifaceWarn.join('; ')+') — проверьте разъём/контакт диска; ' : '')+'SMART без предупреждений ('+have+' диск.)'+(caution.length?'; замечания: '+caution.join('; '):'') };
}
function smartLines(list){
  var out = [];
  list.forEach(function(d){
    out.push(d.name+' · '+d.size_gb+' ГБ · '+(d.bus||'')+(d.is_system?' · системный':'')+' · '+SMART_LABEL[d.status]+(d.health_pct!=null?' · ресурс '+d.health_pct.toFixed(0)+'%':''));
    out.push('    температура '+(d.temp_c!=null?d.temp_c.toFixed(0)+' °C':'—')+' · наработка '+(d.power_on_hours!=null?d.power_on_hours+' ч':'—')+' · включений '+(d.power_cycles!=null?d.power_cycles:'—')+' · записано '+(d.written_gb!=null?d.written_gb.toFixed(0)+' ГБ':'—'));
    d.attrs.forEach(function(a){ out.push('    '+String(a.id).padStart(3,' ')+' '+a.name+' · тек '+a.current+' худш '+a.worst+' порог '+a.threshold+' raw '+rawText(a)+(a.status!=='ok'?' ['+a.status+']':'')); });
    if (d.nvme){ var h=d.nvme; out.push('    NVMe: резерв '+h.available_spare+'% (порог '+h.spare_threshold+'%), износ '+h.percentage_used+'%, ошибки целостности '+h.media_errors+', небезопасных выключений '+h.unsafe_shutdowns); }
    d.notes.forEach(function(n){ out.push('    ! '+n); });
    d.attrs.forEach(function(a){ if (/Phy Error|CRC Error/i.test(a.name) && a.raw>=100 && a.raw<=9007199254740991) out.push('    ℹ '+a.name+' = '+a.raw+': ошибки обмена по SATA-интерфейсу (проверьте разъём/контакт), на здоровье диска не влияют, но при росте — тревожный признак'); });
  });
  return out;
}
function fmtRaw(a){ var v=a.raw; return v.toString(16).toUpperCase().padStart(12,'0')+' · '+(v<=9007199254740991?String(v):'—'); }
/* Атрибуты 194/190 (Temperature/Airflow Temperature) хранят в raw не одно
   число, а текущую температуру в младшем байте плюс мин/макс историю в
   старших — десятичное значение этого 6-байтного поля выглядит как
   бессмысленный "мусор" (например, 193276477485), хотя парсер уже верно
   берёт из него текущую температуру (d.temp_c = raw & 0xFF). Показываем
   это явно, а не сырое число. */
function rawText(a){
  if ((a.id===194 || a.id===190) && a.raw>0){ return (a.raw & 0xFF)+' °C (мин/макс в старших байтах raw, hex '+a.raw.toString(16).toUpperCase()+')'; }
  return String(a.raw);
}
function fieldSmart(){
  A.smLoad();
  var m = S.sm, disks = m.disks;
  if (!disks) return '<div class="runwrap"><div class="idle"><span class="t">··</span><span>чтение SMART…</span></div></div>';
  if (!disks.length) return '<div class="runwrap"><div class="idle" style="color:var(--err)"><span class="t">--</span><span>'+esc(m.err||'Диски не найдены')+'</span></div></div>';
  var d = disks[Math.min(m.sel, disks.length-1)];
  var tabs = '<div class="control"><div class="opts">'+ disks.map(function(x,i){
    return '<button class="opt'+(m.sel===i?' on':'')+'" onclick="echips.smPick('+i+')"><span class="sdot2 st-'+x.status+'"></span>'+esc(x.name)+' · '+x.size_gb+' ГБ</button>';
  }).join('') +'</div></div>';
  function card(k,v,cls){ return '<div class="stat4"><div class="k">'+k+'</div><div class="v '+(cls||'')+'">'+v+'</div></div>'; }
  var cls = d.status==='good'?'ok':d.status==='unknown'?'none':'err';
  var cards = '<div class="stats4">'+
    card('состояние', SMART_LABEL[d.status]+(d.health_pct!=null?' · '+d.health_pct.toFixed(0)+'%':''), cls)+
    card('температура', d.temp_c!=null ? d.temp_c.toFixed(0)+' °C' : '—', d.temp_c!=null && d.temp_c>=70 ? 'err' : '')+
    card('наработка', d.power_on_hours!=null ? d.power_on_hours+' ч ('+(d.power_on_hours/24).toFixed(0)+' дн.)' : '—')+
    card('включений', d.power_cycles!=null ? d.power_cycles : '—')+'</div>'+
    '<div class="stats4" style="margin-top:8px">'+
    card('записано всего', d.written_gb!=null ? d.written_gb.toFixed(0)+' ГБ' : '—')+
    card('прочитано всего', d.read_gb!=null ? d.read_gb.toFixed(0)+' ГБ' : '—')+
    card('интерфейс / тип', esc((d.bus||'—')+(d.media?' · '+d.media:'')))+
    card('прошивка / серийный', esc((d.firmware||'—')+' · '+(d.serial||'—')))+'</div>';
  var table = '';
  if (d.kind==='ata'){
    table = '<div class="smtable"><div class="smh"><span>ID</span><span>Атрибут</span><span>Текущее</span><span>Худшее</span><span>Порог</span><span>Raw (hex · dec)</span></div>'+
      d.attrs.map(function(a){
        return '<div class="smr"><span><i class="sdot2 st-'+(a.status==='ok'?'good':a.status==='warn'?'caution':'bad')+'"></i>'+String(a.id).padStart(2,'0')+'</span><span>'+esc(a.name)+'</span><span>'+a.current+'</span><span>'+a.worst+'</span><span>'+a.threshold+'</span><span class="mono">'+fmtRaw(a)+'</span></div>';
      }).join('')+'</div>';
  } else if (d.kind==='nvme' && d.nvme){
    var h = d.nvme;
    var rows = [['Критическое предупреждение', '0x'+h.critical_warning.toString(16).toUpperCase().padStart(2,'0'), h.critical_warning?'bad':'good'],
      ['Доступный резерв', h.available_spare+'% (порог '+h.spare_threshold+'%)', h.available_spare<=h.spare_threshold&&h.spare_threshold>0?'bad':'good'],
      ['Использовано ресурса', h.percentage_used+'%', h.percentage_used>=90?'bad':h.percentage_used>=50?'caution':'good'],
      ['Ошибки целостности данных', h.media_errors, h.media_errors?'caution':'good'],
      ['Записей в журнале ошибок', h.error_log_entries, 'good'],
      ['Небезопасных выключений', h.unsafe_shutdowns, 'good'],
      ['Циклов включения', h.power_cycles, 'good'],
      ['Наработка, ч', h.power_on_hours, 'good'],
      ['Прочитано / записано, ГБ', h.data_read_gb.toFixed(0)+' / '+h.data_written_gb.toFixed(0), 'good']];
    table = '<div class="smtable"><div class="smh nv"><span>Параметр NVMe</span><span>Значение</span></div>'+
      rows.map(function(r){ return '<div class="smr nv"><span><i class="sdot2 st-'+r[2]+'"></i>'+r[0]+'</span><span class="mono">'+esc(String(r[1]))+'</span></div>'; }).join('')+'</div>';
  }
  var notes = d.notes.length ? '<div class="kbnote" style="margin-top:8px">'+d.notes.map(esc).join('<br>')+'</div>' : '';
  var reasons = smartReasons(d);
  return '<div class="runwrap">'+tabs+cards+(reasons.length?'<div class="kbnote" style="margin-top:8px;color:var(--err)">Замечания: '+esc(reasons.join('; '))+'</div>':'')+table+notes+
    '<div class="kbnote" style="margin-top:8px">Записано/прочитано у SATA — оценка (LBA × 512 байт), у части производителей единица другая. Автоматическая оценка: «Плохое» — атрибут ниже порога, сбой по данным диска или ресурс ≤10%; «Тревога» — переназначенные/нестабильные секторы, ошибки, ресурс ≤50%.</div></div>';
}

/* ---------- сканирование поверхности ---------- */
var SF_COLS = 600;
var SF_CLASSES = [['<5 мс','#8A8F98'],['<20','#4CAF7D'],['<50','#9BCB5B'],['<150','#F0C24B'],['<500','#FF8A00'],['≥500','#E2574C'],['ошибка','#B0102A']];
function sfClassesHtml(cl){
  return SF_CLASSES.map(function(c,i){
    return '<div class="sfc"><i style="background:'+c[1]+'"></i><span>'+c[0]+'</span><b>'+cl[i]+'</b></div>';
  }).join('');
}
function paintSurface(){
  var f = S.sf;
  var txt = document.getElementById('sf-txt');
  if (txt){
    var pct = f.total>0 ? f.pos/f.total*100 : 0, el=(Date.now()-f.t0)/1000, eta = pct>0.5 && f.running ? el/pct*(100-pct) : null;
    txt.textContent = pct.toFixed(1)+'% · '+f.mbps.toFixed(0)+' МБ/с'+(f.liveMin!=null?' (мин '+f.liveMin.toFixed(0)+', макс '+f.liveMax.toFixed(0)+')':'')+' · '+(f.pos/1024).toFixed(1)+' из '+(f.total/1024).toFixed(1)+' ГБ'+(eta!==null?' · осталось ~'+Math.floor(eta/60)+' мин '+Math.round(eta%60)+' с':'');
  }
  var fill = document.getElementById('sf-fill'); if (fill) fill.style.width = (f.total>0 ? f.pos/f.total*100 : 0)+'%';
  var cl = document.getElementById('sf-cls'); if (cl) cl.innerHTML = sfClassesHtml(f.classes);
  var bad = document.getElementById('sf-bad'); if (bad) bad.textContent = f.bad.length ? 'Нечитаемые блоки (смещение, МБ): '+f.bad.slice(0,40).join(', ')+(f.bad.length>40?' … всего '+f.bad.length:'') : '';
  var cv = document.getElementById('sf-canvas');
  if (cv && cv.getContext){
    var w = cv.clientWidth || 600, h = cv.clientHeight || 170;
    if (cv.width!==w) cv.width = w; if (cv.height!==h) cv.height = h;
    var g = cv.getContext('2d'); g.clearRect(0,0,w,h);
    var max = 1; for (var i=0;i<f.cols.length;i++) if (f.cols[i]>max) max=f.cols[i];
    max *= 1.1;
    g.strokeStyle='rgba(255,255,255,.07)'; g.lineWidth=1;
    for (var k=1;k<4;k++){ var y=h*k/4; g.beginPath(); g.moveTo(0,y); g.lineTo(w,y); g.stroke(); }
    g.fillStyle='rgba(255,138,0,.28)'; g.strokeStyle='#FF8A00'; g.lineWidth=1.5;
    g.beginPath(); var first=true, lastx=0;
    for (var c=0;c<SF_COLS;c++){
      if (f.cols[c]==null) continue;
      var x=c/SF_COLS*w, yy=h-(f.cols[c]/max)*h;
      if (first){ g.moveTo(x,h); g.lineTo(x,yy); first=false; } else g.lineTo(x,yy);
      lastx=x;
    }
    if (!first){ g.stroke(); g.lineTo(lastx,h); g.closePath(); g.fill(); }
    g.fillStyle='rgba(255,255,255,.45)'; g.font='10px JetBrains Mono, monospace';
    g.fillText(max.toFixed(0)+' МБ/с', 6, 12); g.fillText('0', 6, h-4);
    var dk = f.disks && f.disks[f.sel];
    if (dk) g.fillText((f.total>0 ? (f.total/1024).toFixed(1) : ((dk.size_gb*(f.endPct-f.startPct)/100).toFixed(1)))+' ГБ диапазона', w-90, h-4);
    // Метки нечитаемых блоков: bad_offsets_mb — абсолютное смещение на диске
    // (удобно для поиска сектора), а ось графика теперь — доля диапазона
    // скана, поэтому пересчитываем в неё же, а не в абсолютную позицию.
    g.fillStyle='#E2574C';
    if (dk && dk.size_gb>0){
      var rangeStartMb = dk.size_gb*1024*f.startPct/100, rangeEndMb = dk.size_gb*1024*f.endPct/100, rangeMb = rangeEndMb-rangeStartMb;
      f.bad.forEach(function(mb){
        if (rangeMb<=0) return;
        var frac = (mb-rangeStartMb)/rangeMb; if (frac<0 || frac>1) return;
        g.fillRect(frac*w, 0, 2, h);
      });
    }
  }
}
function fieldSurface(){
  A.sfLoad();
  var f = S.sf, disks = f.disks || [], r = f.res;
  var ranges = [['all','Весь диск'],['first100','Первые 100 ГБ'],['first10','Первые 10 ГБ'],['last10','Последние 10 ГБ']];
  var cur = disks[f.sel];
  var rangeLabel = typeof f.range==='number' ? 'первые '+f.range+' ГБ' : (ranges.filter(function(x){ return x[0]===f.range; })[0]||[0,''])[1].toLowerCase();
  var pickers = f.running
    ? '<div class="kbnote">Диск: '+esc(cur?cur.name+' · '+cur.size_gb+' ГБ':'—')+' · диапазон: '+esc(rangeLabel)+'</div>'
    : '<div class="control"><div class="k">Диск</div><div class="opts">'+ (disks.length ? disks.map(function(x,i){
        return '<button class="opt'+(f.sel===i?' on':'')+'" onclick="echips.sfPick('+i+')">'+esc(x.name)+' · '+x.size_gb+' ГБ'+(x.is_system?' · системный':'')+'</button>';
      }).join('') : '<span class="kbnote">'+(f.err?esc(f.err):'опрос дисков…')+'</span>') +'</div></div>'+
      '<div class="control" style="margin-top:12px"><div class="k">Диапазон</div><div class="opts">'+ ranges.map(function(x){
        return '<button class="opt mono'+(f.range===x[0]?' on':'')+'" onclick="echips.sfRange(\''+x[0]+'\')">'+x[1]+'</button>';
      }).join('') +'</div></div>';
  var out = '<div class="runwrap">'+pickers+
    '<div class="runrow" style="margin-top:14px"><button class="btn btn-primary" onclick="echips.sfStart()" '+(f.running||!disks.length?'disabled':'')+'>'+(f.running?'Идёт сканирование…':r?'Повторить':'Запустить')+'</button>'+
    (f.running?'<button class="btn btn-ghost" onclick="echips.sfStop()">Остановить</button>':'')+
    '<span class="n">только чтение: данные не меняются, диск не изнашивается; весь диск — от минут (SSD) до нескольких часов (HDD)</span></div>';
  if (f.running || r){
    out += '<div class="bar" style="margin-top:14px"><div class="fill" id="sf-fill" style="width:'+(r?100:(f.total>0?f.pos/f.total*100:0))+'%"></div></div>'+
      '<div class="mbtext" id="sf-txt"></div>'+
      '<canvas id="sf-canvas" class="sfcanvas"></canvas>'+
      '<div class="sfcls" id="sf-cls">'+sfClassesHtml(f.classes)+'</div>'+
      '<div class="kbnote" id="sf-bad" style="color:var(--err)"></div>';
  }
  if (f.err && disks.length) out += '<div class="idle" style="color:var(--err);margin-top:10px"><span>'+esc(f.err)+'</span></div>';
  if (r){
    var total = r.classes.reduce(function(a,b){ return a+b; }, 0) || 1, bad = r.classes[6]>0, warn = r.classes[5]>0 || r.classes[4]/total>0.01;
    out += '<div class="kbnote" style="margin-top:8px">'+(r.stopped?'Остановлено пользователем. ':'')+'Просканировано '+(r.scanned_mb/1024).toFixed(1)+' ГБ за '+r.elapsed_secs+' с · скорость средняя '+r.avg_mbps.toFixed(0)+', минимум '+r.min_mbps.toFixed(0)+', максимум '+r.max_mbps.toFixed(0)+' МБ/с. '+
      (bad ? 'Есть нечитаемые блоки — на диске bad-блоки.' : warn ? 'Есть очень медленные блоки — возможна деградация поверхности.' : 'Ошибок и заметных задержек нет.')+'</div>';
  }
  return out+'</div>';
}

function subTabs(c){
  if (!c.group || S.auto.on) return '';
  return '<div class="subtabs">'+ groupTests(c.group).map(function(t){
    var st = statusOf(t.id);
    return '<button class="subtab'+(t.id===c.id?' on':'')+'" onclick="echips.openCat(\''+t.id+'\')"><i class="sdot2 '+(st==='pass'?'st-good':st==='fail'?'st-bad':st==='na'?'':'')+'"></i>'+esc(t.sub||t.name)+'</button>';
  }).join('') +'</div>';
}

/* ---------- экран «Идёт проверка» автопрогона ---------- */
function autoLogPush(text){
  var d = new Date(), t = String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0')+':'+String(d.getSeconds()).padStart(2,'0');
  (S.autoLog = S.autoLog || []).push({ t:t, text:text }); if (S.autoLog.length>40) S.autoLog.shift();
}
function autoFocusActive(){
  var a = S.auto;
  return !!(a && a.on && !a.waiting && !a.stopped && !a.detail && S.screen==='test' && !isInteractive(S.cat));
}
function autoProgress(c){
  // доля выполнения текущего теста (0–100) или null, если тест не сообщает прогресс
  if (c.kind==='memtest' && S.mem.running) return { pct:S.mem.pct, sub:'Проход '+S.mem.pass+' · '+(S.mem.pattern||'подготовка')+' · ошибок '+S.mem.errors };
  if (c.kind==='diskread' && S.dr.running) return { pct:S.dr.pct, sub:S.dr.mbps.toFixed(0)+' МБ/с' };
  if (c.kind==='diskwrite' && S.dw.running) return { pct:S.dw.pct, sub:(S.dw.phase==='read'?'Чтение обратно':'Запись')+' · '+S.dw.mbps.toFixed(0)+' МБ/с' };
  if (c.kind==='surface' && S.sf.running && S.sf.total>0) return { pct:S.sf.pos/S.sf.total*100, sub:S.sf.mbps.toFixed(0)+' МБ/с · '+(S.sf.pos/1024).toFixed(1)+' из '+(S.sf.total/1024).toFixed(1)+' ГБ' };
  return null;
}
function autoTempSvg(){
  var t = S.autoTemps || [];
  if (t.length<2) return '<svg viewBox="0 0 480 120" preserveAspectRatio="none" class="af-chart" id="auto-temp-svg"></svg>';
  var lo = Math.min.apply(null,t)-3, hi = Math.max(Math.max.apply(null,t)+3, lo+12), lim = profile().maxTempC||95;
  var pts = t.map(function(v,i){ return (i/(t.length-1)*480).toFixed(1)+','+(112-(v-lo)/(hi-lo)*104).toFixed(1); }).join(' ');
  return '<svg viewBox="0 0 480 120" preserveAspectRatio="none" class="af-chart" id="auto-temp-svg"><polyline points="'+pts+'" fill="none" stroke="var(--accent)" stroke-width="2" vector-effect="non-scaling-stroke"/>'+
    (lim<hi ? '<line x1="0" x2="480" y1="'+(112-(lim-lo)/(hi-lo)*104).toFixed(1)+'" y2="'+(112-(lim-lo)/(hi-lo)*104).toFixed(1)+'" stroke="var(--err)" stroke-dasharray="4 4" opacity=".6"/>' : '')+'</svg>';
}
function autoPaintTemp(){
  var v = document.getElementById('auto-temp-v'), svg = document.getElementById('auto-temp-svg');
  if (!v || !S.autoTemps || !S.autoTemps.length) return;
  v.innerHTML = Math.round(S.autoTemps[S.autoTemps.length-1])+'<small> °C</small>';
  if (svg) svg.outerHTML = autoTempSvg();
}
/* Стресс-тест внутри автопрогона — тот же экран «Идёт проверка», данные берутся из тиков стресс-теста */
function autoStressFocusActive(){
  var a = S.auto;
  return !!(a && a.on && !a.waiting && !a.stopped && !a.detail && S.screen==='stress' && S.st.running);
}
/* Датчики внутри автопрогона — тот же экран «Идёт проверка» */
function autoSensorsFocusActive(){
  var a = S.auto;
  return !!(a && a.on && !a.waiting && !a.stopped && !a.detail && S.screen==='sensors');
}
function afLineSvg(series, opts){
  // общий график для экрана «Идёт проверка»: несколько рядов, каждый со своей шкалой
  var W = 480, H = 120, out = '';
  series.forEach(function(sr){
    var d = (sr.data||[]).filter(function(v){ return v!=null; }).slice(-120);
    if (d.length<2) return;
    var lo = sr.min!=null ? sr.min : Math.min.apply(null,d)-3, hi = sr.max!=null ? sr.max : Math.max(Math.max.apply(null,d)+3, lo+12);
    if (hi<=lo) hi = lo+1;
    var pts = d.map(function(v,i){ return (i/(d.length-1)*W).toFixed(1)+','+(H-8-(Math.min(hi,Math.max(lo,v))-lo)/(hi-lo)*(H-16)).toFixed(1); }).join(' ');
    out += '<polyline points="'+pts+'" fill="none" stroke="'+sr.color+'" stroke-width="2" vector-effect="non-scaling-stroke"'+(sr.dash?' stroke-dasharray="5 4"':'')+'/>';
    if (sr.limit!=null && sr.limit<hi && sr.limit>lo){ var y = (H-8-(sr.limit-lo)/(hi-lo)*(H-16)).toFixed(1); out += '<line x1="0" x2="'+W+'" y1="'+y+'" y2="'+y+'" stroke="var(--err)" stroke-dasharray="4 4" opacity=".6"/>'; }
  });
  return '<svg viewBox="0 0 '+W+' '+H+'" preserveAspectRatio="none" class="af-chart'+(opts&&opts.tall?' tall':'')+'">'+out+'</svg>';
}
function screenAutoFocusSensors(){
  var a = S.auto, n = a.ids.length, r = S.sensorReading;
  var secs = profile().sensorsProbeSecs || 8, el = Math.max(0, (Date.now()-(a.stepT0||Date.now()))/1000), pct = Math.min(100, el/secs*100);
  var cnt = { pass:0, fail:0, na:0 }; a.ids.forEach(function(id){ var x = S.results[id]; if (cnt[x]!==undefined) cnt[x]++; });
  var cpuT = r && r.available && r.cpu_temp_c!=null ? Math.round(r.cpu_temp_c) : null, lim = profile().maxTempC||95;
  var gpus = gpuFromSnap(S.hwm && S.hwm.snap);
  var rows = gpus.map(function(g){
    var bits = []; if (g.load!=null) bits.push('загрузка '+g.load.toFixed(0)+'%'); if (g.clockCore!=null) bits.push(g.clockCore.toFixed(0)+' МГц'); if (g.power!=null && g.power>0) bits.push(g.power.toFixed(1)+' Вт');
    return '<div class="af-row"><b>'+esc(g.vendor)+(g.temp!=null ? ' · '+g.temp.toFixed(0)+' °C' : '')+'</b><span>'+esc(g.name)+(bits.length?' · '+bits.join(' · '):'')+'</span></div>';
  }).join('') || '<div class="af-row"><span>Видеоадаптеры датчиками не обнаружены</span></div>';
  var hist = S.sensorHistory.length ? S.sensorHistory : S.gpuHistory;
  var modeName = a.mode==='express' ? 'Экспресс' : 'Полный автопрогон';
  return '<div class="pane af">'+
    '<div class="af-head"><div><div class="eyebrow">'+modeName+' · шаг '+(a.idx+1)+' из '+n+'</div><h1 class="title">Датчики</h1></div>'+
    '<div class="headactions"><button class="btn btn-ghost" onclick="echips.autoDetail()">Подробности теста</button><button class="btn btn-ghost" onclick="echips.autoStop()">Прервать автопрогон</button></div></div>'+
    '<div class="af-ticks">'+afTicksHtml()+'</div>'+
    '<div class="af-grid">'+
      '<section class="af-pan af-big"><h3>Снимаем показания температурных датчиков</h3><div class="af-pct">'+Math.round(pct)+'<small>%</small></div><div class="bar"><div class="fill" style="width:'+pct.toFixed(0)+'%"></div></div>'+
        '<p class="af-mut">'+esc(a.msg || 'Опрос датчиков…')+'</p>'+
        '<div class="af-mini"><div><b>'+fmtTime(Math.round(el))+' / '+fmtTime(secs)+'</b><span>время замера</span></div><div><b>'+hist.length+'</b><span>замеров</span></div><div><b>'+cnt.pass+'</b><span>тестов пройдено</span></div></div></section>'+
      '<section class="af-pan"><h3>Температура процессора</h3><div class="af-tv">'+(cpuT!=null ? cpuT+'<small> °C</small>' : '—')+'</div>'+afLineSvg([{ data:hist, color:'var(--accent)', limit:lim }])+'<p class="af-mut">'+(r && r.note ? esc(r.note) : 'порог '+lim+' °C')+'</p></section>'+
      '<section class="af-pan"><h3>Видеоадаптеры</h3><div class="af-rows">'+rows+'</div></section>'+
    '</div>'+
    '<section class="af-pan af-log"><h3>Журнал</h3>'+((S.autoLog||[]).slice(-5).map(function(l){ return '<div><span>'+esc(l.t)+'</span>'+esc(l.text)+'</div>'; }).join('') || '<div><span></span>Ждём результатов…</div>')+'</section></div>';
}
function afTicksHtml(){
  var a = S.auto;
  return a.ids.map(function(id,i){
    var r = S.results[id], cl = i===a.idx ? 'run' : (r==='pass'||r==='fail'||r==='na') ? r : (i<a.idx ? 'na' : '');
    var nm = (CATS.filter(function(x){ return x.id===id; })[0]||{}).name || id;
    return '<i class="af-tk '+cl+'" title="'+esc(nm)+'"></i>';
  }).join('');
}
function afThrottle(h){
  var c = h.clock.filter(function(v){ return v!=null; });
  if (c.length<10 || !h.clockMax) return false;
  var top = Math.max.apply(null, c.slice(0, Math.max(5, Math.floor(c.length/3))));   // «эталон» — начало нагрузки
  var low = c.slice(Math.floor(c.length/3)).filter(function(v){ return v < top*0.8; }).length;
  return low>=3;
}
function afScoreRows(p, st){
  var rows = [], sc = p.scores || {}, names = { cpu:'CPU, Мопс/с', fpu:'FPU, ГФлопс', cache:'Кэш', memory:'Память', disk:'Диск', gpu:'GPU, кадр/с' };
  Object.keys(sc).forEach(function(k){
    var base = st.hist.scores && st.hist.scores[k], arr = base && base.length ? base : null;
    var avg = arr ? arr.reduce(function(x,y){ return x+y; },0)/arr.length : null;
    rows.push('<div class="af-row"><b>'+esc(names[k]||k)+'</b><span>сейчас '+Math.round(sc[k]*10)/10+(avg!=null ? ' · среднее '+avg.toFixed(1)+' · мин '+Math.min.apply(null,arr).toFixed(1) : '')+'</span></div>');
  });
  if (p.gpuTempC!=null) rows.push('<div class="af-row"><b>Видеокарта</b><span>'+Math.round(p.gpuTempC)+' °C</span></div>');
  if (p.powerW) rows.push('<div class="af-row"><b>Мощность</b><span>'+p.powerW.toFixed(0)+' Вт</span></div>');
  if (p.fanRpm) rows.push('<div class="af-row"><b>Вентилятор</b><span>'+Math.round(p.fanRpm)+' об/мин</span></div>');
  return rows.join('') || '<div class="af-row"><span>Разогрев…</span></div>';
}
function screenAutoFocusStress(){
  var a = S.auto, st = S.st, p = st.last || {}, h = st.hist, n = a.ids.length;
  var dur = st.cfg.dur || 0, pct = dur>0 ? Math.min(100, (st.elapsed||0)/dur*100) : null;
  var cnt = { pass:0, fail:0, na:0 }; a.ids.forEach(function(id){ var r = S.results[id]; if (cnt[r]!==undefined) cnt[r]++; });
  var temps = h.temp.filter(function(v){ return v!=null; }), lim = profile().maxTempC||95;
  var svg = '<svg viewBox="0 0 480 120" preserveAspectRatio="none" class="af-chart"></svg>';
  if (temps.length>1){
    var lo = Math.min.apply(null,temps)-3, hi = Math.max(Math.max.apply(null,temps)+3, lo+12);
    var pts = temps.slice(-120).map(function(v,i,arr){ return (i/(arr.length-1)*480).toFixed(1)+','+(112-(v-lo)/(hi-lo)*104).toFixed(1); }).join(' ');
    svg = '<svg viewBox="0 0 480 120" preserveAspectRatio="none" class="af-chart"><polyline points="'+pts+'" fill="none" stroke="var(--accent)" stroke-width="2" vector-effect="non-scaling-stroke"/>'+
      (lim<hi ? '<line x1="0" x2="480" y1="'+(112-(lim-lo)/(hi-lo)*104).toFixed(1)+'" y2="'+(112-(lim-lo)/(hi-lo)*104).toFixed(1)+'" stroke="var(--err)" stroke-dasharray="4 4" opacity=".6"/>' : '')+'</svg>';
  }
  var loads = h.load.slice(-28), bars = loads.map(function(v){ return '<i style="height:'+Math.max(4, v).toFixed(0)+'%"></i>'; }).join('');
  var sc = p.scores ? Object.keys(p.scores).map(function(k){ return k.toUpperCase()+' '+Math.round(p.scores[k]); }).join(' · ') : '';
  var ev = st.events.slice(-7).map(function(t){ return '<div><span></span>'+esc(t)+'</div>'; }).join('') || '<div><span></span>Нагрузка идёт, событий нет.</div>';
  var errs = (p.memErrors||0)+(p.diskErrors||0);
  var modeName = a.mode==='express' ? 'Экспресс' : 'Полный автопрогон';
  return '<div class="pane af">'+
    '<div class="af-head"><div><div class="eyebrow">'+modeName+' · шаг '+(a.idx+1)+' из '+n+'</div><h1 class="title">Стресс-тест</h1></div>'+
    '<div class="headactions"><button class="btn btn-ghost" onclick="echips.autoDetail()">Подробности теста</button><button class="btn btn-ghost" onclick="echips.autoStop()">Прервать автопрогон</button></div></div>'+
    '<div class="af-ticks">'+afTicksHtml()+'</div>'+
    '<div class="af-grid">'+
      '<section class="af-pan af-big"><h3>Нагрузка на процессор'+(st.cfg.gpu?' и видеокарту':'')+' — проверка охлаждения и троттлинга</h3>'+
        (pct!=null ? '<div class="af-pct">'+Math.round(pct)+'<small>%</small></div><div class="bar"><div class="fill" style="width:'+pct.toFixed(0)+'%"></div></div>' : '<div class="af-pct af-wait">Идёт нагрузка</div><div class="bar ind"><div class="fill"></div></div>')+
        '<p class="af-mut">'+esc(sc || 'Разогрев…')+(p.clockMhz ? ' · '+Math.round(p.clockMhz)+' МГц' : '')+'</p>'+
        '<div class="af-mini"><div><b>'+fmtTime(st.elapsed||0)+(dur?' / '+fmtTime(dur):'')+'</b><span>время нагрузки</span></div><div><b>'+errs+'</b><span>ошибок данных</span></div><div><b>'+cnt.pass+'</b><span>тестов пройдено</span></div></div></section>'+
      '<section class="af-pan"><h3>Температура процессора</h3><div class="af-tv">'+(p.tempC!=null ? Math.round(p.tempC)+'<small> °C</small>' : '—')+'</div>'+svg+'<p class="af-mut">порог '+lim+' °C'+(p.gpuTempC!=null ? ' · видеокарта '+Math.round(p.gpuTempC)+' °C' : '')+'</p></section>'+
      '<section class="af-pan"><h3>Загрузка</h3><div class="af-tv">'+(p.load!=null ? Math.round(p.load)+'<small> %</small>' : '—')+'</div><div class="af-bars">'+bars+'</div><p class="af-mut">'+(p.fanRpm ? 'вентилятор '+Math.round(p.fanRpm)+' об/мин' : '')+(p.powerW ? (p.fanRpm?' · ':'')+p.powerW.toFixed(0)+' Вт' : '')+'</p></section>'+
      '<section class="af-pan af-wide"><h3>Частота процессора и загрузка</h3><div class="af-tv">'+(p.clockMhz ? Math.round(p.clockMhz)+'<small> МГц</small>' : '—')+(st.hist.clockMax ? '<small class="af-side"> макс. '+Math.round(st.hist.clockMax)+' МГц · ' : '')+(st.hist.clockMax ? (p.clockMhz ? Math.round(p.clockMhz/st.hist.clockMax*100) : 0)+'% от максимума</small>' : '')+'</div>'+
        afLineSvg([{ data:h.clock, color:'var(--cool)', min:0, max:st.hist.clockMax||null }, { data:h.load, color:'var(--accent)', min:0, max:100, dash:true }], { tall:true })+
        '<p class="af-mut"><i class="af-key" style="background:var(--cool)"></i>частота &nbsp; <i class="af-key" style="background:var(--accent)"></i>загрузка'+(afThrottle(h) ? ' &nbsp;·&nbsp; <b class="af-warn">частота падала ниже 80% — возможен троттлинг</b>' : '')+'</p></section>'+
      '<section class="af-pan"><h3>Результаты нагрузки</h3><div class="af-rows">'+afScoreRows(p, st)+'</div></section>'+
    '</div>'+
    '<section class="af-pan af-log"><h3>События нагрузки</h3>'+ev+'</section></div>';
}
function screenAutoFocus(){
  var a = S.auto, c = cat(), n = a.ids.length;
  var pr = autoProgress(c);
  var cnt = { pass:0, fail:0, na:0 }; a.ids.forEach(function(id){ var r = S.results[id]; if (cnt[r]!==undefined) cnt[r]++; });
  var ticks = a.ids.map(function(id,i){
    var r = S.results[id], cl = i===a.idx ? 'run' : (r==='pass'||r==='fail'||r==='na') ? r : (i<a.idx ? 'na' : '');
    var nm = (CATS.filter(function(x){ return x.id===id; })[0]||{}).name || id;
    return '<i class="af-tk '+cl+'" title="'+esc(nm)+'"></i>';
  }).join('');
  var el = Math.max(0, Math.round((Date.now()-(a.stepT0||Date.now()))/1000));
  var last = S.autoTemps && S.autoTemps.length ? Math.round(S.autoTemps[S.autoTemps.length-1]) : null;
  var log = (S.autoLog||[]).slice(-7).map(function(l){ return '<div><span>'+esc(l.t)+'</span>'+esc(l.text)+'</div>'; }).join('') || '<div><span></span>Ждём результатов первых проверок…</div>';
  var modeName = a.mode==='express' ? 'Экспресс' : 'Полный автопрогон';
  return '<div class="pane af">'+
    '<div class="af-head"><div><div class="eyebrow">'+modeName+' · шаг '+(a.idx+1)+' из '+n+'</div><h1 class="title">'+esc(c.name)+'</h1></div>'+
    '<div class="headactions"><button class="btn btn-ghost" onclick="echips.autoDetail()">Подробности теста</button><button class="btn btn-ghost" onclick="echips.autoStop()">Прервать автопрогон</button></div></div>'+
    '<div class="af-ticks">'+ticks+'</div>'+
    '<div class="af-grid">'+
      '<section class="af-pan af-big"><h3>'+esc(c.method)+'</h3>'+
        (pr ? '<div class="af-pct">'+Math.round(pr.pct)+'<small>%</small></div><div class="bar"><div class="fill" style="width:'+pr.pct.toFixed(0)+'%"></div></div><p class="af-mut">'+esc(pr.sub)+'</p>'
            : '<div class="af-pct af-wait">Идёт проверка</div><div class="bar ind"><div class="fill"></div></div><p class="af-mut">'+esc(a.msg || 'Идёт проверка')+'</p>')+
        '<div class="af-mini"><div><b>'+fmtTime(el)+'</b><span>идёт этот тест</span></div><div><b>'+cnt.pass+'</b><span>пройдено</span></div><div><b>'+cnt.fail+'</b><span>ошибок</span></div></div></section>'+
      '<section class="af-pan"><h3>Температура процессора</h3><div class="af-tv" id="auto-temp-v">'+(last!=null ? last+'<small> °C</small>' : '—')+'</div>'+autoTempSvg()+'<p class="af-mut">порог '+(profile().maxTempC||95)+' °C</p></section>'+
      '<section class="af-pan"><h3>Ход автопрогона</h3><div class="af-tv">'+(a.idx+1)+'<small> из '+n+'</small></div><div class="af-mini af-col"><div><b>'+(n-a.idx-1)+'</b><span>осталось тестов</span></div><div><b>'+cnt.na+'</b><span>не применимо</span></div></div></section>'+
    '</div>'+
    '<section class="af-pan af-log"><h3>Журнал</h3>'+log+'</section></div>';
}
setInterval(function(){ if (autoFocusActive() || autoSensorsFocusActive()) render(); }, 1000);   // тикает время шага на экране «Идёт проверка»
function autoBanner(){
  var a = S.auto; if (!a.on) return '';
  var n = a.ids.length;
  var btns = '<button class="btn btn-ghost" onclick="echips.autoStop()">Прервать автопрогон</button>';
  if (a.detail && !a.waiting && !a.stopped && !isInteractive(S.cat)) btns = '<button class="btn btn-ghost" onclick="echips.autoDetail()">Обзор</button>'+btns;
  if (a.stopped) btns = '<button class="btn btn-ghost" onclick="echips.autoNext()">Продолжить</button><button class="btn btn-primary" onclick="echips.autoReport()">К отчёту</button>';
  else if (a.waiting) btns = '<button class="btn btn-primary" onclick="echips.autoNext()">Далее</button>' + btns;
  return '<div class="autobar"><div class="ab-top"><span class="eyebrow">Автопрогон · профиль «'+esc(profile().name)+'»</span>'+
    '<span class="idx">шаг '+(a.idx+1)+' из '+n+'</span></div>'+
    '<div class="bar"><div class="fill" style="width:'+(a.idx/n*100).toFixed(0)+'%"></div></div>'+
    '<div class="ab-row">'+(a.msg ? '<div class="ab-msg '+a.cls+'">'+esc(a.msg)+'</div>' : '<div class="ab-msg"></div>')+
    '<div class="headactions">'+btns+'</div></div></div>';
}

function sparkInner(samples){
  var MAXBARS = 120, arr = samples;
  if (arr.length > MAXBARS){
    var k = arr.length / MAXBARS; arr = [];
    for (var i=0;i<MAXBARS;i++){ var a=Math.floor(i*k), b=Math.max(a+1, Math.floor((i+1)*k)), sum=0; for (var j=a;j<b;j++) sum+=samples[j]; arr.push(sum/(b-a)); }
  }
  var max = Math.max.apply(null, arr.concat([1]));
  return arr.map(function(v){ return '<i style="height:'+Math.max(3, v/max*100).toFixed(0)+'%" title="'+v.toFixed(0)+' МБ/с"></i>'; }).join('');
}
function sparkBars(samples){ return '<div class="spark">'+sparkInner(samples)+'</div>'; }
function fieldDiskRead(){
  A.drLoad();
  var d = S.dr, disks = d.disks || [], r = d.res;
  var modes = [[64,'Быстрый · 1,5 ГБ'],[128,'3 ГБ'],[256,'Расширенный · 6 ГБ'],[512,'12 ГБ']];
  var out = '<div class="runwrap">'+
    '<div class="control"><div class="k">Диск</div><div class="opts">'+ (disks.length ? disks.map(function(x,i){
      return '<button class="opt'+(d.sel===i?' on':'')+'" onclick="echips.drPick('+i+')" '+(d.running?'disabled':'')+'>'+esc(x.name)+' · '+x.size_gb+' ГБ'+(x.is_system?' · системный':'')+'</button>';
    }).join('') : '<span class="kbnote">'+(d.err?esc(d.err):'опрос дисков…')+'</span>') +'</div></div>'+
    '<div class="control" style="margin-top:12px"><div class="k">Режим</div><div class="opts">'+ modes.map(function(m){
      return '<button class="opt mono'+(d.mode===m[0]?' on':'')+'" onclick="echips.drMode('+m[0]+')" '+(d.running?'disabled':'')+'>'+m[1]+'</button>';
    }).join('') +'</div></div>'+
    '<div class="runrow" style="margin-top:14px"><button class="btn btn-primary" onclick="echips.drStart()" '+(d.running||!disks.length?'disabled':'')+'>'+(d.running?'Идёт чтение…':r?'Повторить':'Запустить')+'</button>'+
    (d.running?'<button class="btn btn-ghost" onclick="echips.drStop()">Остановить</button>':'')+
    '<span class="n">чтение 24 участков по всему диску, запись не выполняется</span></div>';
  if (d.running || r){
    out += '<div class="bar" style="margin-top:14px"><div class="fill" id="dr-fill" style="width:'+(r?100:d.pct)+'%"></div></div>'+
      '<div class="mbtext" id="dr-txt">'+(d.running ? d.pct+'% · '+d.mbps.toFixed(0)+' МБ/с' : '')+'</div>';
  }
  if (d.err && disks.length) out += '<div class="idle" style="color:var(--err);margin-top:10px"><span>'+esc(d.err)+'</span></div>';
  if (r){
    var drop = r.max_mbps>0 ? Math.round((1-r.min_mbps/r.max_mbps)*100) : 0;
    var bad = r.errors>0, warn = r.slow_blocks>0 || drop>60;
    out += sparkBars(r.samples)+
      '<div class="stats4">'+
      '<div class="stat4"><div class="k">средняя</div><div class="v">'+r.avg_mbps.toFixed(0)+' МБ/с</div></div>'+
      '<div class="stat4"><div class="k">мин / макс</div><div class="v">'+r.min_mbps.toFixed(0)+' / '+r.max_mbps.toFixed(0)+'</div></div>'+
      '<div class="stat4"><div class="k">медленных блоков</div><div class="v '+(r.slow_blocks?'err':'ok')+'">'+r.slow_blocks+'</div></div>'+
      '<div class="stat4"><div class="k">ошибок чтения</div><div class="v '+(bad?'err':'ok')+'">'+r.errors+'</div></div></div>'+
      '<div class="kbnote" style="margin-top:8px">'+(r.stopped?'Остановлено пользователем. ':'')+'Прочитано '+r.read_mb+' МБ. '+
      (bad ? 'Есть ошибки чтения (смещения, МБ: '+r.error_offsets_mb.join(', ')+') — диск неисправен.' :
       warn ? 'Есть медленные участки или просадка скорости '+drop+'% — возможна деградация диска.' : 'Ошибок и заметных просадок нет.')+'</div>';
  }
  return out+'</div>';
}
function fieldBrightness(){
  A.brLoad();
  var b = S.br.info;
  if (!b) return '<div class="runwrap"><div class="idle"><span class="t">··</span><span>опрос подсветки…</span></div></div>';
  if (!b.available) return '<div class="runwrap"><div class="idle"><span class="t">--</span><span>Управление яркостью через WMI недоступно: настольный ПК, внешний монитор или драйвер не поддерживает. Отметьте «Не применимо».</span></div></div>';
  return '<div class="runwrap"><div class="control"><div class="k">Яркость подсветки: <span id="br-val" style="color:var(--text)">'+b.current+'%</span></div>'+
    '<input type="range" class="range" min="'+b.min+'" max="'+b.max+'" value="'+b.current+'" oninput="echips.brInput(this.value)"></div>'+
    '<div class="runrow" style="margin-top:14px"><button class="btn btn-ghost" onclick="echips.brSet('+b.min+')">Минимум</button>'+
    '<button class="btn btn-ghost" onclick="echips.brSet('+Math.round((b.min+b.max)/2)+')">50%</button>'+
    '<button class="btn btn-ghost" onclick="echips.brSet('+b.max+')">Максимум</button>'+
    '<span class="n">яркость матрицы должна плавно и без мерцания меняться</span></div></div>';
}
/* Класс порта по скорости чтения с флешки (запись у дешёвых флешек медленная и для порта
   показательна хуже). Пороги ориентировочные: реальный потолок USB 2.0 ≈ 30–40 МБ/с,
   USB 3.x — сотни; 45–60 — неоднозначная зона (медленная флешка или порт). */
function usbClass(r){
  var rd = r.read_mbps;
  if (rd >= 60) return { txt:'порт уровня USB 3.x', cls:'ok' };
  if (rd >= 45) return { txt:'неоднозначно: медленная флешка или порт USB 2.0/3.x', cls:'' };
  return { txt:'скорость уровня USB 2.0 (для синего порта USB 3.x — проверьте порт и флешку)', cls:'warn' };
}
function fieldRemovable(){
  A.rmStart();
  var rm = S.rm, drives = rm.drives;
  var out = '<div class="runwrap"><div class="kbnote" style="margin-bottom:10px">Вставьте флешку в проверяемый порт — она появится в списке. Для проверки нескольких портов вставляйте её по очереди.</div>';
  if (drives===null) out += '<div class="idle"><span class="t">··</span><span>поиск накопителей…</span></div>';
  else if (!drives.length) out += '<div class="idle"><span class="t">--</span><span>'+(rm.err?esc(rm.err):'USB-накопитель не найден — вставьте флешку')+'</span></div>';
  else out += '<div class="devlist2">'+drives.map(function(d){
      return '<div class="devrow2"><span class="dot" style="background:var(--ok);box-shadow:0 0 8px var(--ok)"></span>'+esc(d.letter)+': '+esc(d.label||'без метки')+' · '+d.size_gb+' ГБ · свободно '+d.free_gb+' ГБ · '+esc(d.fs)+
        '<button class="btn btn-primary" style="margin-left:auto;padding:7px 14px" onclick="echips.rmTest(\''+esc(d.letter)+'\')" '+(rm.running?'disabled':'')+'>'+(rm.running===d.letter?'Проверка…':'Проверить запись/чтение')+'</button></div>';
    }).join('')+'</div>';
  if (rm.log.length) out += '<div class="log">'+rm.log.map(function(r,i){
      return r.error ? '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span style="color:var(--err)">'+esc(r.letter)+': '+esc(r.error)+'</span></div>'
        : '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span>'+esc(r.letter)+': запись '+r.write_mbps.toFixed(1)+' МБ/с · чтение '+r.read_mbps.toFixed(1)+' МБ/с · '+r.size_mb+' МБ · '+(r.errors?'<b style="color:var(--err)">ошибок данных: '+r.errors+'</b>':'данные совпали')+' · <span style="color:'+(usbClass(r).cls==='ok'?'var(--ok)':usbClass(r).cls==='warn'?'#F0C24B':'var(--mute)')+'">'+usbClass(r).txt+'</span></span></div>';
    }).join('')+'</div>';
  return out+'</div>';
}
function fieldDiskWrite(){
  A.dwLoad();
  var d = S.dw, vols = d.vols || [], r = d.res;
  var modes = [[512,'512 МБ'],[2048,'2 ГБ'],[10240,'10 ГБ'],[51200,'50 ГБ'],[102400,'100 ГБ'],[0,'Максимум']];
  var curV = vols[d.sel];
  var modeLabel = (modes.filter(function(m){ return m[0]===d.mb; })[0]||[0,d.mb+' МБ'])[1];
  var pickersW = d.running
    ? '<div class="kbnote">Том: '+esc(curV?curV.letter+': '+(curV.label||'без метки')+' · '+curV.size_gb+' ГБ':'—')+' · файл: '+esc(modeLabel)+'</div>'
    : '<div class="control"><div class="k">Том</div><div class="opts">'+ (vols.length ? vols.map(function(v,i){
        return '<button class="opt'+(d.sel===i?' on':'')+'" onclick="echips.dwPick('+i+')">'+esc(v.letter)+': '+esc(v.label||'без метки')+' · '+v.size_gb+' ГБ, свободно '+v.free_gb+(v.is_system?' · системный':'')+'</button>';
      }).join('') : '<span class="kbnote">'+(d.err?esc(d.err):'опрос томов…')+'</span>') +'</div></div>'+
      '<div class="control" style="margin-top:12px"><div class="k">Размер проверочного файла</div><div class="opts">'+ modes.map(function(m){
        return '<button class="opt mono'+(d.mb===m[0]?' on':'')+'" onclick="echips.dwMode('+m[0]+')">'+m[1]+'</button>';
      }).join('') +'</div></div>';
  var out = '<div class="runwrap">'+pickersW+
    '<div class="runrow" style="margin-top:14px"><button class="btn btn-primary" onclick="echips.dwStart()" '+(d.running||!vols.length?'disabled':'')+'>'+(d.running?'Идёт запись…':r?'Повторить':'Запустить')+'</button>'+
    (d.running?'<button class="btn btn-ghost" onclick="echips.dwStop()">Остановить</button>':'')+
    '<span class="n">пишется временный файл, данные на диске не затрагиваются; файл удаляется после теста</span></div>'+
    (d.mb===0||d.mb>=10240 ? '<div class="kbnote" style="margin-top:8px">Большие объёмы: '+(d.mb===0?'до 85% свободного места':(d.mb/1024)+' ГБ')+' записи. Для SSD с ресурсом 300 ТБ 100 ГБ — около 0,03% ресурса, диск это не убивает, но время теста растёт (запись + проверка чтением).</div>' : '');
  if (d.running || r){
    out += '<div class="bar" style="margin-top:14px"><div class="fill" id="dw-fill" style="width:'+(r?100:d.pct)+'%"></div></div>'+
      '<div class="mbtext" id="dw-txt">'+(d.running ? d.pct+'% · '+(d.phase==='write'?'запись':'чтение')+' · '+d.mbps.toFixed(0)+' МБ/с' : '')+'</div>'+
      (d.running ? '<div class="spark" id="dw-live">'+sparkInner(d.live)+'</div>' : '');
  }
  if (d.err && vols.length) out += '<div class="idle" style="color:var(--err);margin-top:10px"><span>'+esc(d.err)+'</span></div>';
  if (r){
    var bad = r.errors>0, warn = r.slow_blocks>3;
    out += sparkBars(r.write_samples)+
      '<div class="stats4">'+
      '<div class="stat4"><div class="k">запись, средняя</div><div class="v">'+r.write_avg_mbps.toFixed(0)+' МБ/с</div></div>'+
      '<div class="stat4"><div class="k">запись мин / макс</div><div class="v">'+r.write_min_mbps.toFixed(0)+' / '+r.write_max_mbps.toFixed(0)+'</div></div>'+
      '<div class="stat4"><div class="k">чтение обратно</div><div class="v">'+r.read_mbps.toFixed(0)+' МБ/с</div></div>'+
      '<div class="stat4"><div class="k">ошибок данных</div><div class="v '+(bad?'err':'ok')+'">'+r.errors+'</div></div></div>'+
      '<div class="kbnote" style="margin-top:8px">'+(r.stopped?'Остановлено пользователем. ':'')+
      (bad ? 'Данные прочитались не так, как записаны — диск или контроллер неисправны.' :
       warn ? 'Медленных блоков: '+r.slow_blocks+' — возможна деградация диска или перегрев SSD.' : 'Данные совпали, заметных просадок нет.')+'</div>';
  }
  return out+'</div>';
}
function fieldMem(){
  var m = S.mem, r = m.res;
  var out = '<div class="runwrap">'+
    '<div class="control"><div class="k">Проходов</div><div class="opts">'+ [1,2,4].map(function(n){
      return '<button class="opt mono'+(m.passes===n?' on':'')+'" onclick="echips.memPasses('+n+')" '+(m.running?'disabled':'')+'>'+n+'</button>';
    }).join('') +'</div></div>'+
    '<div class="runrow" style="margin-top:14px"><button class="btn btn-primary" onclick="echips.memStart()" '+(m.running?'disabled':'')+'>'+(m.running?'Идёт проверка…':r?'Повторить':'Запустить')+'</button>'+
    (m.running?'<button class="btn btn-ghost" onclick="echips.memStop()">Остановить</button>':'')+
    '<span class="n">проверяется вся свободная память (запас ~10% остаётся системе); окно остаётся отзывчивым</span></div>';
  if (m.running || r){
    var elF = (Date.now()-m.t0)/1000, etaF = m.running && m.pct>1 ? elF/m.pct*(100-m.pct) : null;
    out += '<div class="bar" style="margin-top:14px"><div class="fill" id="mem-fill" style="width:'+(r?100:m.pct)+'%"></div></div>'+
      '<div class="mbtext" id="mem-txt">'+(m.running ? m.pct+'% · проход '+m.pass+' · '+esc(m.pattern)+' · ошибок '+m.errors+(etaF!=null?' · осталось ~'+Math.max(0,Math.round(etaF))+' с':'') : '')+'</div>';
  }
  if (m.err) out += '<div class="idle" style="color:var(--err);margin-top:10px"><span>'+esc(m.err)+'</span></div>';
  if (r){
    out += '<div class="stats4" style="margin-top:14px">'+
      '<div class="stat4"><div class="k">проверено</div><div class="v">'+r.tested_mb+' МБ</div></div>'+
      '<div class="stat4"><div class="k">проходов</div><div class="v">'+r.passes+'</div></div>'+
      '<div class="stat4"><div class="k">ошибок</div><div class="v '+(r.errors?'err':'ok')+'">'+r.errors+'</div></div>'+
      '<div class="stat4"><div class="k">время</div><div class="v">'+r.elapsed_secs+' с</div></div></div>'+
      '<div class="kbnote" style="margin-top:8px">'+(r.stopped?'Остановлено пользователем. ':'')+
      (r.errors ? 'Обнаружены ошибки памяти — модуль или слот неисправны.' : 'Ошибок не найдено. Это быстрая проверка из-под Windows: для полной уверенности используйте длительный тест.')+'</div>'+
      (r.first_errors.length ? '<div class="log" style="margin-top:8px">'+r.first_errors.map(function(t,i){ return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span>'+esc(t)+'</span></div>'; }).join('')+'</div>' : '');
  }
  return out+'</div>';
}

/* Подсказка «что делать», если тест не пройден (автооценка или вердикт техника). Тексты — profile().hints,
   поэтому правятся в data/profiles.json без пересборки. */
function hintFor(id){ var H = profile().hints || {}; return H[id] || ''; }
function hintBox(id){
  var d = S.detail[id], failed = S.results[id]==='fail' || (d && d.auto && d.auto.status==='fail') || (S.cat===id && S.verdict && S.verdict.status==='fail');
  var t = failed ? hintFor(id) : '';
  return t ? '<div class="hintbox"><b>Что делать:</b> '+esc(t)+'</div>' : '';
}
function screenTest(){
  if (autoFocusActive()) return screenAutoFocus();
  var c = cat(), field = '';
  if(c.kind==='keyboard') field = fieldKeyboard();
  else if(c.kind==='display') field = fieldDisplay();
  else if(c.kind==='touchpad') field = fieldTouchpad();
  else if(c.kind==='camera') field = fieldCamera();
  else if(c.kind==='audio') field = fieldAudio();
  else if(c.kind==='headset') field = fieldHeadset();
  else if(c.kind==='touch') field = fieldTouch();
  else if(c.kind==='diskread') field = fieldDiskRead();
  else if(c.kind==='diskwrite') field = fieldDiskWrite();
  else if(c.kind==='fans') field = fieldFans();
  else if(c.kind==='smart') field = fieldSmart();
  else if(c.kind==='surface') field = fieldSurface();
  else if(c.kind==='memtest') field = fieldMem();
  else if(c.kind==='brightness') field = fieldBrightness();
  else if(c.kind==='removable') field = fieldRemovable();
  else field = fieldRunner();
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'dash\')">← все категории</button>'+
    '<span class="idx">категория '+(CATS.indexOf(c)+1)+' из '+CATS.length+'</span></div>'+
    autoBanner()+subTabs(c)+
    '<div class="testhead"><div><h2>'+c.name+'</h2><div class="hint">'+c.method+'</div></div>'+
    '<div class="base">'+c.tag+' · '+c.impl+'</div></div>'+
    '<div class="field">'+field+'</div>'+
    (S.markErr && S.markErr.id===c.id ? '<div class="markerr" id="mark-err">'+esc(S.markErr.text)+'</div>' : '')+
    hintBox(c.id)+
    '<div class="verdict">'+
      '<input placeholder="Комментарий техника — попадёт в отчёт" value="'+esc(S.comments[c.id]||'')+'" oninput="echips.comment(this.value)">'+
      '<button class="btn btn-ghost" onclick="echips.mark(\'na\')" title="Такого узла нет в этой модели (например, тачпад на настольном ПК)">Не применимо</button>'+
      '<button class="btn btn-danger" onclick="echips.mark(\'fail\')">Не пройден</button>'+
      '<button class="btn btn-primary" onclick="echips.mark(\'pass\')">Пройден</button>'+
    '</div></div>';
}

var HW_UNITS = { Temperature:'°C', Fan:'об/мин', Voltage:'В', Power:'Вт', Clock:'МГц', Load:'%', Control:'%', Level:'%', Data:'ГБ', SmallData:'МБ', Current:'А', Energy:'мВт·ч', Frequency:'Гц', Flow:'л/ч', Factor:'', Throughput:'Б/с', TimeSpan:'с', Noise:'дБА' };
var HW_TYPES = { Temperature:'Температура', Fan:'Вентилятор', Voltage:'Напряжение', Power:'Мощность', Clock:'Частота', Load:'Загрузка', Control:'Управление', Level:'Уровень', Data:'Данные', SmallData:'Данные', Current:'Ток', Energy:'Энергия', Frequency:'Частота', Flow:'Поток', Factor:'Коэффициент', Throughput:'Скорость', TimeSpan:'Время', Noise:'Шум' };
function hwmonPanel(){
  var h = S.hwm, st = h.status, sn = h.snap;
  if (!st) return '<div class="actpanel"><div class="kbnote">LibreHardwareMonitor: проверка…</div></div>';
  var out = '<div class="actpanel"><div class="kbnote" style="margin-bottom:8px"><b style="color:var(--text)">LibreHardwareMonitor</b> — температуры процессора, обороты вентиляторов, напряжения и мощность. Нужен драйвер PawnIO: он вшит в программу и ставится сам при первом запуске (без вопросов); удалить можно кнопкой ниже.</div>';
  if (!st.embedded){
    return out+'<div class="kbnote" style="color:var(--err)">Датчики не вшиты в эту сборку (локальная сборка). Используйте exe из релиза.</div></div>';
  }
  var line = 'Драйвер PawnIO: '+(st.driverInstalled ? '<b style="color:var(--ok)">установлен</b>' : '<b style="color:var(--err)">не установлен</b>')+' · датчики: '+(st.running ? (st.hasData ? '<b style="color:var(--ok)">работают, показаний: '+st.sensors+'</b>' : 'запущены, ждём данные…') : 'не запущены');
  out += '<div class="kbnote" style="margin-bottom:8px">'+line+(st.message ? ' · '+esc(st.message) : '')+'</div>';
  if (h.busy){
    out += '<div class="kbnote" style="color:var(--accent-hi)">'+(h.confirm==='uninstall' ? 'Удаление драйвера PawnIO…' : 'Установка драйвера PawnIO… (несколько секунд)')+'</div>';
  } else if (h.confirm==='uninstall'){
    out += '<div class="actconfirm">'+'Удалить драйвер PawnIO из системы? Температуры процессора станут недоступны, автоустановка при запуске отключится.'+
      '<div class="headactions" style="margin-top:10px"><button class="btn btn-ghost" onclick="echips.hwmCancel()" '+(h.busy?'disabled':'')+'>Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.hwmRun()" '+(h.busy?'disabled':'')+'>'+(h.busy?'Выполняется…':'Да, выполнить')+'</button></div></div>';
  } else {
    out += '<div class="runrow" style="flex-wrap:wrap">'+
      (st.driverInstalled
        ? (FEATURE_PAWNIO_UNINSTALL ? '<button class="btn btn-ghost" onclick="echips.hwmAsk(\'uninstall\')">Удалить драйвер PawnIO</button>' : '')
        : (st.driverEmbedded ? '<button class="btn btn-primary" onclick="echips.hwmInstall()">Установить драйвер PawnIO</button>' : ''))+
      (st.driverInstalled && !st.running ? '<button class="btn btn-ghost" onclick="echips.hwmStart()">Запустить датчики</button>' : '')+'</div>';
  }
  if (h.msg) out += '<div class="kbnote" style="margin-top:8px;color:var(--ok)">'+esc(h.msg)+'</div>';
  if (h.err) out += '<div class="kbnote" style="margin-top:8px;color:var(--err)">'+esc(h.err)+'</div>';
  if (sn && sn.ok && sn.sensors.length){
    var groups = {}, order = [];
    sn.sensors.forEach(function(x){ if (!groups[x.hw]){ groups[x.hw] = []; order.push(x.hw); } groups[x.hw].push(x); });
    out += '<div class="smtable" style="margin-top:12px">'+ order.map(function(name){
      var rows = groups[name].filter(function(x){ return x.type!=='Clock' && x.type!=='Load' && x.type!=='Data' && x.type!=='SmallData' || /Total|Package|Core #1$|CPU Core|Memory|GPU Core/.test(x.name); });
      return '<div class="smh nv"><span>'+esc(name)+' · '+esc(groups[name][0].hwType)+'</span><span></span></div>'+
        rows.map(function(x){
          var u = HW_UNITS[x.type]!==undefined ? HW_UNITS[x.type] : '', v = x.type==='Fan' || x.type==='Clock' ? x.value.toFixed(0) : x.value.toFixed(x.type==='Voltage' ? 3 : 1);
          return '<div class="smr nv"><span>'+esc((HW_TYPES[x.type]||x.type)+' · '+x.name)+'</span><span class="mono">'+v+' '+u+(x.max!=null && x.type==='Temperature' ? ' (макс '+x.max.toFixed(0)+')' : '')+'</span></div>';
        }).join('');
    }).join('') +'</div>';
  } else if (st.driverInstalled && st.running){
    out += '<div class="kbnote" style="margin-top:8px">Показания ещё не пришли — подождите пару секунд.</div>';
  }
  return out+'</div>';
}

/* ----- вентиляторы ----- */
function cssId(id){ return String(id).replace(/[^A-Za-z0-9]/g,'_'); }
function cpuTempFromSnap(sn){
  var t = sn.sensors.filter(function(s){ return /^Cpu/.test(s.hwType) && s.type==='Temperature'; });
  if (!t.length) return null;
  var p = t.filter(function(s){ return /Package|Tctl|Tdie/i.test(s.name); })[0];
  return p ? p.value : Math.max.apply(null, t.map(function(s){ return s.value; }));
}
/* Вентиляторы (датчики Fan) и их управление: пара Fan/Control с одним индексом в одном устройстве. */
function fanList(sn){
  function key(id){ return id.replace(/\/(fan|control)\/(\d+)$/, '/#/$2'); }
  var controls = {};
  sn.sensors.forEach(function(s){ if (s.type==='Control' && s.controllable) controls[key(s.id)] = s; });
  return sn.sensors.filter(function(s){ return s.type==='Fan'; }).map(function(s){
    var c = controls[key(s.id)];
    return { id:s.id, name:s.name, hw:s.hw, rpm:s.value, control: c ? { id:c.id, pct:c.value } : null };
  });
}
function noteFans(sn){
  if (!sn || !sn.ok) return;
  fanList(sn).forEach(function(x){
    var e = S.fan.seen[x.id] || (S.fan.seen[x.id] = { min:x.rpm, max:x.rpm });
    e.min = Math.min(e.min, x.rpm); e.max = Math.max(e.max, x.rpm);
  });
}
function fanRowsHtml(){
  var sn = S.hwm.snap;
  if (!sn || !sn.ok) return '<div class="idle"><span class="t">··</span><span>Датчики не отвечают. Драйвер PawnIO ставится сам при первом запуске; статус — на вкладке «Датчики».</span></div>';
  var fans = fanList(sn);
  if (!fans.length) return '<div class="idle"><span class="t">--</span><span>Вентиляторы датчиками не обнаружены (у части ноутбуков контроллер EC не поддерживается LibreHardwareMonitor).</span></div>';
  return '<div class="smtable fantable"><div class="smh"><span>Вентилятор</span><span>Обороты</span><span>Min / Max за сеанс</span><span>Управление</span></div>'+
    fans.map(function(x){
      var seen = S.fan.seen[x.id] || { min:x.rpm, max:x.rpm }, man = S.fan.manual[x.control ? x.control.id : ''];
      var ctl = x.control
        ? '<input type="range" class="range" min="20" max="100" step="5" value="'+(man!=null ? man : Math.round(x.control.pct))+'" oninput="echips.fanManual(\''+x.control.id.replace(/'/g,'')+'\',this.value)" '+(S.fan.running?'disabled':'')+'> <b id="fan-pct-'+cssId(x.control.id)+'">'+(man!=null ? man : Math.round(x.control.pct))+'%</b>'
        : '<span class="mono">нет ручного управления</span>';
      return '<div class="smr"><span>'+esc(x.name)+' <span class="mono">· '+esc(x.hw)+'</span></span><span class="mono"><b style="color:var(--text)">'+x.rpm.toFixed(0)+'</b> об/мин</span><span class="mono">'+seen.min.toFixed(0)+' / '+seen.max.toFixed(0)+'</span><span>'+ctl+'</span></div>';
    }).join('')+'</div>';
}
function paintFans(){
  var rows = document.getElementById('fan-rows');
  // во время перетаскивания ползунка не перерисовываем таблицу
  if (rows && !document.querySelector('#fan-rows input:active')){
    var focus = document.activeElement; if (!(focus && focus.closest && focus.closest('#fan-rows'))) rows.innerHTML = fanRowsHtml();
  }
  var lg = document.getElementById('fan-log');
  if (lg) lg.innerHTML = S.fan.log.map(function(t,i){ return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span style="white-space:pre-wrap">'+esc(t)+'</span></div>'; }).join('');
}
function fieldFans(){
  A.fanStart();
  var f = S.fan;
  return '<div class="runwrap"><div class="kbnote">Обороты берутся из датчиков LibreHardwareMonitor (драйвер PawnIO). «Проверка отклика» по очереди ставит управляемым вентиляторам 100% и 35% и сравнивает обороты; после проверки авторежим возвращается сам. Ползунки — ручная скорость как в SpeedFan (20–100%), при выходе с экрана авторежим возвращается.</div>'+
    '<div id="fan-rows">'+fanRowsHtml()+'</div>'+
    '<div class="runrow"><button class="btn btn-primary" onclick="echips.fanTest(true)" '+(f.running?'disabled':'')+'>'+(f.running?'Идёт проверка…':'Проверка отклика (активная)')+'</button>'+
    '<button class="btn btn-ghost" onclick="echips.fanTest(false)" '+(f.running?'disabled':'')+'>Пассивная проверка</button>'+
    (f.running ? '<button class="btn btn-ghost" onclick="echips.fanStop()">Остановить</button>' : '<button class="btn btn-ghost" onclick="echips.fanAuto()">Вернуть авторежим</button>')+'</div>'+
    '<div class="log" id="fan-log" style="min-height:90px">'+f.log.map(function(t,i){ return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span style="white-space:pre-wrap">'+esc(t)+'</span></div>'; }).join('')+'</div>'+
    (f.res ? '<div class="kbnote" style="margin-top:6px">Автооценка: '+({pass:'пройден',fail:'не пройден',na:'не применимо'}[f.res.status])+' — '+esc(f.res.note)+'</div>' : '')+'</div>';
}

/* Видеоадаптеры из снимка LibreHardwareMonitor (NVIDIA, AMD, Intel — в т.ч. встроенные). */
function gpuFromSnap(sn){
  if (!sn || !sn.sensors) return [];
  var by = {}, order = [];
  sn.sensors.forEach(function(x){ if (/^Gpu/.test(x.hwType)){ if (!by[x.hw]){ by[x.hw] = { name:x.hw, hwType:x.hwType, s:[] }; order.push(x.hw); } by[x.hw].s.push(x); } });
  return order.map(function(n){
    var g = by[n], s = g.s;
    function pick(type, re){ var l = s.filter(function(x){ return x.type===type && (!re || re.test(x.name)); }); return l.length ? l[0].value : null; }
    function maxOf(type){ var l = s.filter(function(x){ return x.type===type; }).map(function(x){ return x.value; }); return l.length ? Math.max.apply(null, l) : null; }
    var temp = pick('Temperature', /GPU Core|Core/i); if (temp==null) temp = maxOf('Temperature');
    var mem = s.filter(function(x){ return /Memory Used/i.test(x.name) && (x.type==='SmallData' || x.type==='Data'); })[0];
    return { name:g.name, hwType:g.hwType, vendor: /Nvidia/i.test(g.hwType) ? 'NVIDIA' : /Amd/i.test(g.hwType) ? 'AMD' : /Intel/i.test(g.hwType) ? 'Intel' : 'GPU',
      temp:(temp!=null && temp>0 && temp<150) ? temp : null, load: pick('Load', /GPU Core|D3D 3D|Core/i), power: maxOf('Power'),
      clockCore: pick('Clock', /GPU Core|Core/i), clockMem: pick('Clock', /Memory/i), memUsed: mem ? mem.value : null, memType: mem ? mem.type : null };
  });
}
function gpuHottest(sn){ var t = gpuFromSnap(sn).map(function(g){ return g.temp; }).filter(function(v){ return v!=null; }); return t.length ? Math.max.apply(null, t) : null; }
/* ---------- История отчётов (админ) ---------- */
var HIST_ST = { pass:'OK', fail:'ОШИБКА', na:'н/п', idle:'—' };
function histStage(ref){ var m = String(ref.file||'').match(/_(before|after)(?:_(?:full|express))?\.json$/); return m ? m[1] : ''; }
function histMode(ref){ var m = String(ref.file||'').match(/_(full|express)\.json$/); return m ? m[1] : ''; }
function histStageBadge(ref){ var st = histStage(ref), md = histMode(ref); return (st ? ' <span class="stagebadge '+st+'">'+(st==='before'?'до':'после')+'</span>' : '')+(md ? ' <span class="stagebadge">'+(md==='express'?'экспресс':'полный')+'</span>' : ''); }
/* Пара «до / после» в текущем списке: самый свежий «до» и самый свежий «после» с тем же номером приёмки (или серийником) */
function histFiltered(H){
  var q = (H.q||'').toLowerCase().trim();
  return (H.list||[]).filter(function(x){
    if (q && (x.device+' '+x.engineer+' '+x.date).toLowerCase().indexOf(q)<0) return false;
    var m = H.sum && H.sum[x.path];
    if (H.fModel && !(m && m.model===H.fModel)) return false;
    if (H.fMode && !(m && (m.mode||'—')===H.fMode)) return false;
    if (H.fErr && !(m && m.fail>0)) return false;
    return true;
  });
}
function histFindPair(list){
  var bef = list.filter(function(x){ return histStage(x)==='before'; }), aft = list.filter(function(x){ return histStage(x)==='after'; });
  // пара с одинаковым номером приёмки (или серийником); сначала ищем пару ОДНОГО режима (полный/экспресс), потом любую
  for (var pass=0; pass<2; pass++){
    for (var i=0;i<bef.length;i++){
      var b = histLabel(bef[i]);
      for (var j=0;j<aft.length;j++){
        var a = histLabel(aft[j]);
        var same = (b.intake && b.intake===a.intake) || (!b.intake && !a.intake && b.serial===a.serial);
        if (!same) continue;
        var mb = histMode(bef[i]), ma = histMode(aft[j]);
        if (pass===0 && mb && ma && mb!==ma) continue;
        return { before:bef[i], after:aft[j], intake:b.intake||a.intake, serial:b.serial, modeDiff: !!(mb && ma && mb!==ma) };
      }
    }
  }
  return null;
}
function histLabel(ref){
  var m = String(ref.device||'').match(/^(\d{1,6})_(.+)$/);
  return { intake: m ? m[1] : '', serial: m ? m[2] : ref.device };
}
/* Числовые показатели теста из строк подробностей: «износ 0%», «температура 41 °C» → { ключ: {v, unit} }.
   Ключ = название теста + текст перед числом; время/наработка/счётчики включений в сравнение не берём. */
var HIST_SKIP = /наработк|включен|записан|длительност|время|проход|потоков|модул|замеров|ядер|ошибок чтения:? *0$/i;
function histMetrics(res){
  var out = {}, lines = (res.details||[]).concat([res.auto_note||'']);
  lines.forEach(function(line){
    String(line).split(/ · |; |, /).forEach(function(part){
      var m = part.match(/^\s*[•✓✗⚠]?\s*([^\d:=]*?[A-Za-zА-Яа-я][^\d:=]*?)[:=]?\s*(-?\d+(?:[.,]\d+)?)\s*(%|°C|МБ\/с|ГБ\/с|ГБ|МБ|МГц|ГГц|Вт|мс|об\/мин|ГФлопс|Мопс\/с|кадр\/с)?\s*$/);
      if (!m) return;
      var key = m[1].replace(/\s+/g,' ').trim(), unit = m[3] || '';
      if (key.length<3 || HIST_SKIP.test(key)) return;
      var k = res.title+' · '+key; if (out[k]===undefined) out[k] = { v:parseFloat(m[2].replace(',','.')), unit:unit, key:key, title:res.title };
    });
  });
  return out;
}
function histMetricDiff(ra, rb){
  var A = {}, B = {}, rows = [];
  (ra.results||[]).forEach(function(r){ var m = histMetrics(r); Object.keys(m).forEach(function(k){ A[k] = m[k]; }); });
  (rb.results||[]).forEach(function(r){ var m = histMetrics(r); Object.keys(m).forEach(function(k){ B[k] = m[k]; }); });
  Object.keys(B).forEach(function(k){
    if (A[k] && A[k].v!==B[k].v) rows.push({ title:B[k].title, key:B[k].key, a:A[k].v, b:B[k].v, unit:B[k].unit || A[k].unit });
  });
  return rows;
}
function histSummary(rep){
  var c = { pass:0, fail:0, na:0, idle:0 };
  (rep.results||[]).forEach(function(r){ c[r.status] = (c[r.status]||0)+1; });
  return c;
}
function histHeader(env){
  var r = env.report||{}, c = histSummary(r);
  return '<div class="card"><div class="k">'+esc(r.device_model||'—')+'</div>'+
    '<div class="s" style="margin-top:6px">SN '+esc(r.device_serial||'—')+(r.intake ? ' · приёмка/ремонт '+esc(r.intake) : '')+' · инженер '+esc(r.engineer||'—')+(r.run_mode ? ' · режим '+esc(r.run_mode) : '')+'</div>'+
    '<div class="s">'+esc(r.started_at||'')+' → '+esc(r.finished_at||'')+' · версия '+esc(env.app_version||'?')+' · отправка: '+esc(env.kind||'')+'</div>'+
    '<div class="s" style="margin-top:6px">пройдено '+c.pass+' · ошибок '+c.fail+' · не применимо '+c.na+' · не проверено '+c.idle+'</div>'+
    (r.summary_comment ? '<div class="s" style="margin-top:6px">Комментарий: '+esc(r.summary_comment)+'</div>' : '')+'</div>';
}
/* Читаемый отчёт для администратора — по образцу отчёта после автопрогона: итог, счётчики, проверки по областям
   (клик по строке раскрывает подробности из отчёта), справа заключение инженера, сведения и скачивание. */
var ST_LABEL = { pass:'пройден', fail:'не пройден', na:'не применимо', idle:'не проверялся' };
function fmtDur(a, b){
  var x = new Date(a).getTime(), y = new Date(b).getTime(); if (isNaN(x) || isNaN(y) || y<x) return '';
  var m = Math.round((y-x)/60000); return m<60 ? m+' мин' : Math.floor(m/60)+' ч '+(m%60)+' мин';
}
function histReportView(H){
  var env = H.view.env, r = env.report||{}, res = r.results||[], c = histSummary(r);
  var inProf = res.filter(function(x){ return x.in_profile!==false; });
  var pending = inProf.filter(function(x){ return x.status==='idle'; }).length;
  var verdict = c.fail ? 'Нужен ремонт' : pending ? 'Проверка не завершена' : 'Годен';
  var cls = c.fail ? 'fail' : pending ? 'idle' : 'pass';
  var failed = res.filter(function(x){ return x.status==='fail'; });
  var sub = failed.length ? failed.map(function(x){ return x.title+(x.auto_note||x.comment ? ': '+String(x.comment||x.auto_note).slice(0,90) : ''); }).slice(0,2).join('. ')+'.' : (pending ? 'Не проверено тестов профиля: '+pending+'.' : 'Все проверки профиля выполнены, ошибок нет.');
  var byId = {}; res.forEach(function(x){ byId[x.id] = x; });
  var used = {}, sections = DASH_AREAS.map(function(a){
    var rows = a[1].map(function(id){ used[id] = 1; return byId[id]; }).filter(Boolean);
    return [a[0], rows];
  });
  var rest = res.filter(function(x){ return !used[x.id]; });
  if (rest.length) sections.push(['Прочее', rest]);
  H.open = H.open || {};
  function row(x){
    var st = x.status, out = st==='idle' && x.in_profile===false, open = !!H.open[x.id];
    var note = String(x.comment||x.auto_note||''), det = x.details||[];
    var extra = open ? '<tr class="hv-d"><td></td><td colspan="3">'+
      (x.override_reason ? '<div class="kbnote" style="color:var(--err)">Вердикт изменён инженером: '+esc(x.override_reason)+'</div>' : '')+
      (x.auto_note && x.comment && x.comment!==x.auto_note ? '<div class="kbnote">Автооценка: '+esc(x.auto_note)+'</div>' : '')+
      (note ? '<div class="kbnote" style="color:var(--text-2)">'+esc(note)+'</div>' : '')+
      (det.length ? '<div class="log" style="margin-top:8px">'+det.map(function(t,i){ return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span style="white-space:pre-wrap">'+esc(t)+'</span></div>'; }).join('')+'</div>' : '<div class="kbnote">Подробностей в отчёте нет.</div>')+
      (x.finished_at ? '<div class="kbnote" style="margin-top:6px">завершён '+esc(new Date(x.finished_at).toLocaleString('ru-RU'))+'</div>' : '')+
      '</td></tr>' : '';
    return '<tr class="'+st+' clickable" onclick="echips.histToggle(\''+esc(x.id)+'\')"><td><i class="ddot '+st+'"></i></td><td class="rn">'+esc(x.title)+'</td><td class="rl">'+(out?'вне профиля':ST_LABEL[st]||st)+'</td>'+
      '<td class="rr">'+esc(note.length>110 ? note.slice(0,108)+'…' : note)+'</td></tr>'+extra;
  }
  var table = '<table class="rp-tb"><thead><tr><th></th><th>Проверка</th><th>Статус</th><th>Результат и комментарий</th></tr></thead><tbody>'+
    sections.map(function(sec){ return '<tr class="hv-h"><td colspan="4">'+esc(sec[0])+'</td></tr>'+sec[1].map(row).join(''); }).join('')+'</tbody></table>';
  var dur = fmtDur(r.started_at, r.finished_at);
  var stage = r.repair_stage==='before' ? 'до ремонта' : r.repair_stage==='after' ? 'после ремонта' : '';
  var kv = function(k, v){ return v ? '<div class="hv-kv"><span>'+k+'</span><b>'+esc(v)+'</b></div>' : ''; };
  return '<div class="headactions" style="margin-bottom:12px"><button class="btn btn-ghost" onclick="echips.histBack()">← к списку</button></div>'+
    '<div class="pane rp" style="padding:0">'+
    '<div class="rp-head"><div class="rp-v '+cls+'"><p class="rp-k">'+esc(r.device_model||'Устройство')+' · SN '+esc(r.device_serial||'—')+'</p><h1>'+verdict+'</h1><p class="rp-sub">'+esc(sub)+'</p></div>'+
      '<div class="rp-c"><div class="dst pass"><b>'+c.pass+'</b><span>пройдено</span></div><div class="dst fail"><b>'+c.fail+'</b><span>ошибка</span></div><div class="dst na"><b>'+c.na+'</b><span>не применимо</span></div></div>'+
      '<div class="headactions"><button class="btn btn-ghost" onclick="echips.histSave(\'txt\')" '+(H.saving?'disabled':'')+'>TXT</button><button class="btn btn-ghost" onclick="echips.histSave(\'json\')" '+(H.saving?'disabled':'')+'>JSON</button><button class="btn btn-primary" onclick="echips.histSave(\'pdf\')" '+(H.saving?'disabled':'')+'>'+(H.saving==='pdf'?'Готовлю PDF…':'Скачать PDF')+'</button></div></div>'+
    (H.err ? '<div class="kbnote" style="color:var(--err);margin-bottom:10px">'+esc(H.err)+'</div>' : '')+
    (H.saved ? '<div class="infoline" style="margin:0 0 12px">Сохранён: <span class="mono">'+esc(H.saved.path)+'</span> <button class="btn-link" onclick="echips.histReveal()">показать в папке</button></div>' : '')+
    '<div class="rp-wrap"><div>'+table+'</div><aside class="rp-side"><h3>Заключение инженера</h3>'+
      (r.summary_comment ? '<p style="white-space:pre-wrap;margin:0 0 18px">'+esc(r.summary_comment)+'</p>' : '<p class="rs-mut">Инженер не оставил заключения.</p>')+
      '<h3>Сведения</h3>'+kv('Инженер', r.engineer)+kv('Приёмка / ремонт', r.intake)+kv('Этап', stage)+kv('Режим', r.run_mode)+
        kv('Начало', r.started_at ? new Date(r.started_at).toLocaleString('ru-RU') : '')+kv('Окончание', r.finished_at ? new Date(r.finished_at).toLocaleString('ru-RU') : '')+kv('Длительность', dur)+
        kv('Версия программы', env.app_version)+kv('Отправка', env.kind==='auto' ? 'конец автопрогона' : env.kind==='manual' ? 'экспорт' : env.kind==='sync' ? 'автосохранение' : env.kind)+
    '</aside></div></div>';
}

/* Журнал сервера (только админ): входы, начало и конец диагностики, кто и когда. Контроль: «начал и не закончил». */
var EV_LABEL = { login:'вход', login_fail:'неверный PIN', login_blocked:'блокировка входа', report:'отчёт получен', auto_start:'старт автопрогона', auto_end:'автопрогон завершён', auto_abort:'автопрогон прерван', user_save:'инженер сохранён', user_delete:'инженер удалён', profile_save:'профиль сохранён' };
function evTime(t){ var d = new Date(t*1000); return String(d.getHours()).padStart(2,'0')+':'+String(d.getMinutes()).padStart(2,'0')+':'+String(d.getSeconds()).padStart(2,'0'); }
/* Дни журнала считаются по московскому времени (UTC+3) — так же группирует события сервер */
function evLocalDay(){ return new Date(Date.now()+3*3600e3).toISOString().slice(0,10); }
function screenEvents(){
  var E = S.events = S.events || { date:evLocalDay(), list:null, loading:false, err:'' };
  var list = E.list || [], names = {}, byUser = {};
  list.forEach(function(e){ if (e.type==='login' && e.data && e.data.machine) names[e.data.machine] = e.data.name || ''; });
  list.forEach(function(e){
    var u = byUser[e.user||'—'] = byUser[e.user||'—'] || { login:0, start:0, end:0, abort:0, reports:0, fail:0 };
    if (e.type==='login') u.login++; else if (e.type==='auto_start') u.start++; else if (e.type==='auto_end') u.end++; else if (e.type==='auto_abort') u.abort++; else if (e.type==='report') u.reports++; else if (e.type==='login_fail') u.fail++;
  });
  var sum = Object.keys(byUser).map(function(k){
    var u = byUser[k], lost = Math.max(0, u.start-u.end-u.abort);
    return '<tr><td class="rn">'+esc(k)+'</td><td class="mono">'+u.login+'</td><td class="mono">'+u.start+'</td><td class="mono">'+u.end+'</td><td class="mono">'+u.abort+'</td><td class="mono">'+u.reports+'</td><td class="mono">'+(lost ? '<b style="color:var(--err)">'+lost+'</b>' : '0')+'</td></tr>';
  }).join('');
  var rows = list.slice().reverse().map(function(e){
    var d = e.data||{}, det = [];
    if (d.machine) det.push('ПК: '+(names[d.machine] || d.machine));
    if (d.model) det.push(d.model); if (d.serial) det.push('SN '+d.serial);
    if (d.mode) det.push(d.mode==='express' ? 'экспресс' : 'полный'); if (d.pass!==undefined) det.push('✓'+d.pass+' ✗'+d.fail);
    if (d.at) det.push('на шаге '+d.at+' из '+d.of); if (d.path) det.push(d.path); if (d.id) det.push(d.id);
    var bad = e.type==='login_fail' || e.type==='login_blocked' || e.type==='auto_abort';
    return '<tr'+(bad?' class="fail"':'')+'><td class="mono rl">'+evTime(e.t)+'</td><td class="rn">'+esc(EV_LABEL[e.type]||e.type)+'</td><td>'+esc(e.user||'—')+'</td><td class="rr">'+esc(det.join(' · '))+'</td><td class="mono rl">'+esc(e.ip||'')+'</td></tr>';
  }).join('');
  return '<div class="pane te"><div class="af-head"><div><div class="eyebrow">Только для администратора</div><h1 class="title">Журнал</h1></div>'+
    '<div class="headactions"><input type="date" class="search-input" style="max-width:170px" value="'+esc(E.date)+'" onchange="echips.evDate(this.value)"><button class="btn btn-ghost" onclick="echips.evLoad()" '+(E.loading?'disabled':'')+'>Обновить</button></div></div>'+
    (E.err ? '<div class="kbnote" style="color:var(--err);margin-bottom:10px">'+esc(E.err)+'</div>' : '')+
    (E.loading ? '<div class="kbnote">загрузка…</div>' : !list.length ? '<p class="rs-mut">За этот день событий нет.</p>' :
      '<h3 style="margin:6px 0 10px">По инженерам</h3><table class="rp-tb"><thead><tr><th>Инженер</th><th>Входов</th><th>Стартов</th><th>Завершено</th><th>Прервано</th><th>Отчётов</th><th title="Начал автопрогон, но не завершил и не прервал (закрыл программу, выключился ПК)">Не закончено</th></tr></thead><tbody>'+sum+'</tbody></table>'+
      '<h3 style="margin:22px 0 10px">Все события</h3><table class="rp-tb"><thead><tr><th>Время</th><th>Событие</th><th>Инженер</th><th>Подробности</th><th>IP</th></tr></thead><tbody>'+rows+'</tbody></table>')+
    '</div>';
}

function screenHistory(){
  var H = S.hist || { list:null, loading:false, err:'', q:'', sel:[], view:null, cmp:null, busy:false, sum:{}, sumBusy:false, fModel:'', fMode:'', fErr:false };
  var body;
  if (H.busy){
    body = hexSpinner('ЗАГРУЗКА ОТЧЁТА');
  } else if (H.view){
    body = histReportView(H);
  } else if (H.cmp){
    var A_ = H.cmp.a.env, B_ = H.cmp.b.env, ra = A_.report||{}, rb = B_.report||{}, byId = {};
    (ra.results||[]).forEach(function(x){ byId[x.id] = { a:x }; });
    (rb.results||[]).forEach(function(x){ (byId[x.id] = byId[x.id] || {}).b = x; });
// «не проверено» (idle) и отсутствие теста изменением не считаем: экспресс и полный прогон дают разный набор
    var rows = Object.keys(byId).map(function(id){ var e = byId[id], sa = e.a && e.a.status, sb = e.b && e.b.status;
      return { title:(e.b||e.a).title, a:e.a, b:e.b, changed: !!sa && !!sb && sa!=='idle' && sb!=='idle' && sa!==sb }; });
    var changed = rows.filter(function(x){ return x.changed; });
    var head = [];
    ['device_model','device_serial','intake','engineer','run_mode'].forEach(function(k){ if ((ra[k]||'')!==(rb[k]||'')) head.push(k+': «'+(ra[k]||'—')+'» → «'+(rb[k]||'—')+'»'); });
    body = '<div class="headactions" style="margin-bottom:12px"><button class="btn btn-ghost" onclick="echips.histBack()">← к списку</button></div>'+
      '<div class="cmpgrid"><div>'+histHeader(A_)+'</div><div>'+histHeader(B_)+'</div></div>'+
      ((ra.run_mode||'') && (rb.run_mode||'') && ra.run_mode!==rb.run_mode ? '<div class="hintbox">Сравниваются отчёты разных режимов («'+esc(ra.run_mode)+'» и «'+esc(rb.run_mode)+'»): часть тестов есть только в полном режиме и не сравнивается. Для точного сравнения запустите «после ремонта» в том же режиме, что и «до».</div>' : '')+
      (head.length ? '<div class="kbnote" style="margin-top:10px">Отличия в заголовке: '+head.map(esc).join(' · ')+'</div>' : '')+
      '<div class="kbnote" style="margin-top:10px">Изменилось тестов: '+changed.length+' из '+rows.length+' (тесты «не проверено» в одном из отчётов не сравниваются)</div>'+
      (function(){
        var md = histMetricDiff(ra, rb);
        return md.length ? '<div class="kbnote" style="margin-top:12px">Изменившиеся показатели ('+md.length+')</div><div class="smtable" style="margin-top:6px">'+md.slice(0,60).map(function(x){
          var d = x.b - x.a, arrow = d>0 ? '▲' : '▼';
          return '<div class="smr nv"><span>'+esc(x.title)+' · '+esc(x.key)+'</span><span class="mono">'+x.a+' → '+x.b+' '+esc(x.unit)+' '+arrow+'</span></div>';
        }).join('')+'</div>' : '';
      })()+
      '<div class="smtable" style="margin-top:8px"><div class="smh nv"><span>Тест</span><span>Старый → новый</span></div>'+
      rows.filter(function(x){ return x.changed || (x.a&&x.a.status==='fail') || (x.b&&x.b.status==='fail'); }).map(function(x){
        var sa = x.a ? HIST_ST[x.a.status] : '—', sb = x.b ? HIST_ST[x.b.status] : '—';
        var note = x.changed && x.b ? String(x.b.comment||x.b.auto_note||'').slice(0,140) : '';
        return '<div class="smr nv"><span>'+esc(x.title)+(x.changed?' <b style="color:var(--accent)">●</b>':'')+'</span><span class="mono">'+sa+' → '+sb+(note?' · '+esc(note):'')+'</span></div>';
      }).join('')+'</div>';
  } else {
    var list = histFiltered(H);
    var models = {}, modes = {};
    Object.keys(H.sum||{}).forEach(function(p){ var m = H.sum[p]; if (m.model) models[m.model] = 1; modes[m.mode||'—'] = 1; });
    function sel(k, items, ph){ return '<select class="search-input" style="max-width:180px" onchange="echips.histFilter(\''+k+'\', this.value)"><option value="">'+ph+'</option>'+Object.keys(items).map(function(v){ return '<option value="'+esc(v)+'"'+(H[k]===v?' selected':'')+'>'+esc(v)+'</option>'; }).join('')+'</select>'; }
    body = '<div class="runrow" style="margin-bottom:10px"><input class="search-input" style="max-width:360px" placeholder="поиск: серийник, приёмка, инженер, дата" value="'+esc(H.q||'')+'" oninput="echips.histQ(this.value)">'+
      '<button class="btn btn-ghost" onclick="echips.histLoad(true)" '+(H.loading?'disabled':'')+'>Обновить</button>'+
      '<button class="btn btn-primary" onclick="echips.histCompare()" '+(H.sel.length===2?'':'disabled')+'>Сравнить выбранные ('+H.sel.length+'/2)</button>'+
      (function(){ var pr = histFindPair(list); return pr ? '<button class="btn btn-primary" onclick="echips.histComparePair()">Сравнить до / после'+(pr.intake?' · '+esc(pr.intake):'')+'</button>' : ''; })()+'</div>'+
      '<div class="runrow" style="margin-bottom:10px">'+sel('fModel', models, 'все модели')+sel('fMode', modes, 'все режимы')+
      '<label class="kbnote"><input type="checkbox" '+(H.fErr?'checked ':'')+'onclick="echips.histFilter(\'fErr\', this.checked)"> только с ошибками</label>'+
      '<button class="btn btn-ghost" onclick="echips.histLoadSums()" '+(H.sumBusy?'disabled':'')+'>'+(H.sumBusy?'Загрузка сводок…':'Загрузить сводки (первые 60)')+'</button>'+
      '<button class="btn btn-ghost" onclick="echips.histExport()">Сохранить список в CSV</button></div>'+
      ((H.fModel||H.fMode||H.fErr) && Object.keys(H.sum||{}).length<(H.list||[]).length ? '<div class="kbnote" style="margin-bottom:8px">Фильтры по модели, режиму и ошибкам работают только для отчётов с загруженной сводкой.</div>' : '')+
      (H.err ? '<div class="kbnote" style="color:var(--err);margin-bottom:8px">'+esc(H.err)+'</div>' : '')+
      (H.msg ? '<div class="kbnote" style="color:var(--ok);margin-bottom:8px">'+esc(H.msg)+'</div>' : '')+
      (H.loading ? '<div class="kbnote">загрузка списка отчётов…</div>' :
      '<div class="kbnote" style="margin-bottom:8px">Отчётов: '+list.length+(list.length>200 ? ' (показаны первые 200 — уточните поиск)' : '')+'</div>'+
      '<div class="smtable"><div class="smh histrow"><span></span><span>Дата</span><span>Инженер</span><span>Приёмка</span><span>Серийный номер</span><span>Итог</span><span></span></div>'+
      (H.shown = list.slice(0,200)).map(function(x,i){
        var d = histLabel(x), on = H.sel.indexOf(x.path)>=0, tm = /^\d{6}/.test(x.file) ? x.file.slice(0,2)+':'+x.file.slice(2,4) : '';
        return '<div class="smr histrow"><span><input type="checkbox" '+(on?'checked ':'')+'onclick="echips.histPick('+i+')"></span>'+
          '<span class="mono">'+esc(x.date)+' '+tm+'</span><span>'+esc(x.engineer)+'</span><span class="mono">'+esc(d.intake||'—')+'</span><span class="mono">'+esc(d.serial)+histStageBadge(x)+'</span>'+
          '<span class="mono">'+(function(){ var m = H.sum[x.path]; return m ? (m.err ? '—' : '<b style="color:var(--ok)">✓'+m.pass+'</b> '+(m.fail?'<b style="color:var(--err)">✗'+m.fail+'</b> ':'')+'<span style="opacity:.6">'+esc(m.mode||'')+'</span>') : ''; })()+'</span>'+
          '<span><button class="btn-link" onclick="echips.histOpen('+i+')">открыть</button></span></div>';
      }).join('')+'</div>');
  }
  return '<div class="pane"><div class="head"><div><div class="eyebrow">Только для администратора</div><h1 class="title">История</h1></div></div>'+
    '<div class="lede">Отчёты из приватного репозитория: открыть, найти по серийнику или приёмке, сравнить два отчёта одного ноутбука (до и после ремонта).</div>'+
    '<div class="field" style="margin-top:14px">'+body+'</div></div>';
}

function screenSensors(){
  var r = S.sensorReading;
  var cpuVal = r && r.available ? r.cpu_temp_c.toFixed(1) : '—';
  var g = r && r.gpu;
  var rows = [
    { k:'CPU (ACPI)', v:cpuVal, u:'°C', c:'#FF8A00', m: r ? esc(r.note) : 'опрос…' },
  ];
  var gpus = gpuFromSnap(S.hwm && S.hwm.snap);
  if (gpus.length){
    gpus.forEach(function(g){
      var integ = g.vendor==='Intel' || (g.vendor==='AMD' && g.temp==null);
      var parts = [];
      if (g.load!=null) parts.push('загрузка '+g.load.toFixed(0)+'%');
      if (g.power!=null && g.power>0) parts.push(g.power.toFixed(1)+' Вт');
      if (g.clockCore!=null) parts.push(g.clockCore.toFixed(0)+' МГц');
      if (g.clockMem!=null) parts.push('память '+g.clockMem.toFixed(0)+' МГц');
      if (g.memUsed!=null) parts.push('видеопамяти занято '+(g.memType==='Data' ? (g.memUsed*1024).toFixed(0) : g.memUsed.toFixed(0))+' МБ');
      if (g.vendor==='NVIDIA' && r && r.gpu){ if (r.gpu.fan_pct!=null) parts.push('вентилятор '+r.gpu.fan_pct+'%'); }
      if (g.temp==null) parts.push(integ ? 'встроенная графика: температура в составе CPU (Package)' : 'температура недоступна');
      rows.push({ k:'GPU · '+g.vendor, v: g.temp!=null ? g.temp.toFixed(0) : '—', u: g.temp!=null ? '°C' : '', c: g.vendor==='NVIDIA' ? '#76B900' : g.vendor==='AMD' ? '#E5484D' : '#4A90E2',
        m: esc(g.name)+(parts.length ? ' · '+parts.join(' · ') : '') });
    });
  } else {
    var g0 = r && r.gpu;
    rows.push({ k:'GPU', v: g0 ? g0.temp_c.toFixed(0) : '—', u:'°C', c:'#6E8FA8',
      m: g0 ? esc(g0.name)+(g0.fan_pct!=null?' · вентилятор '+g0.fan_pct+'%':'')+(g0.power_w!=null?' · '+g0.power_w+' Вт':'')+(g0.util_pct!=null?' · загрузка '+g0.util_pct+'%':'')
            : (r ? 'Видеоадаптеры не обнаружены датчиками — нужен драйвер PawnIO (запустите датчики ниже).' : 'опрос…') });
  }
  var grid = '<line x1="0" y1="0" x2="1000" y2="0" stroke="rgba(255,255,255,.06)"></line>'+
    '<line x1="0" y1="150" x2="1000" y2="150" stroke="rgba(255,255,255,.06)"></line>'+
    '<line x1="0" y1="300" x2="1000" y2="300" stroke="rgba(255,255,255,.10)"></line>';
  var hist = S.sensorHistory.length ? S.sensorHistory : S.gpuHistory;
  var points = hist.length ? hist.map(function(v,i){
    var x = (i/((hist.length-1)||1))*1000;
    var y = 300 - Math.max(0,Math.min(1,(v-20)/(80-20)))*300;
    return x.toFixed(1)+','+y.toFixed(1);
  }).join(' ') : '';
  var chart = hist.length
    ? '<div class="plot"><svg viewBox="0 0 1000 300" preserveAspectRatio="none">'+grid+
      '<polyline points="'+points+'" fill="none" stroke="'+(S.sensorHistory.length?'#FF8A00':'#6E8FA8')+'" stroke-width="2" vector-effect="non-scaling-stroke" stroke-linejoin="round"></polyline></svg></div>'
    : '<div class="infoline" style="margin:0">'+(r && !r.available ? esc(r.note) : 'ожидание данных…')+'</div>';
  return '<div class="pane">'+autoBanner()+
    '<div class="head"><div><div class="eyebrow">Live · WMI ACPI</div><h1 class="title">Датчики</h1></div>'+
    '<div class="lbl" style="font-family:var(--mono);font-size:11px;color:var(--dim);display:flex;align-items:center;gap:8px">'+
    '<span class="pulse'+(S.sensorPoll?' anim':'')+'"></span>опрос 2 с</div></div>'+
    '<div class="readouts">'+ rows.map(function(r2){
      return '<div class="readout"><div class="k"><i style="background:'+r2.c+'"></i>'+r2.k+'</div>'+
        '<div class="v"><b>'+r2.v+'</b><span>'+r2.u+'</span></div><div class="m">'+r2.m+'</div></div>';
    }).join('') +'</div>'+
    '<div class="chart">'+chart+'</div>'+hwmonPanel()+
    '<div class="footrow"><span class="txt">Без драйвера PawnIO доступен один ACPI-датчик через WMI (не на всех платах) и GPU NVIDIA через nvidia-smi; с драйвером — все видеоадаптеры (NVIDIA, AMD, Intel, в т.ч. встроенные). Снимок можно приложить к отчёту.</span>'+
    '<button class="btn btn-ghost" onclick="echips.snapshot()">'+(S.snapshot?'Снимок добавлен в отчёт':'Приложить снимок к отчёту')+'</button></div></div>';
}

/* ---------- стресс-тест ---------- */
var ST_PRESETS = {
  quick:{ label:'Быстрый · 1 мин', cpu:true, fpu:true, cache:false, memory:false, disk:false, gpu:false, dur:60 },
  std:{ label:'Стандарт · 10 мин', cpu:true, fpu:true, cache:true, memory:true, disk:false, gpu:false, dur:600 },
  heat:{ label:'Прогрев до остановки', cpu:true, fpu:true, cache:true, memory:false, disk:false, gpu:false, dur:0 },
  full:{ label:'Всё сразу · 30 мин', cpu:true, fpu:true, cache:true, memory:true, disk:true, gpu:true, dur:1800 },
  runin:{ label:'Прогон 4 часа', cpu:true, fpu:true, cache:true, memory:true, disk:false, gpu:true, dur:14400 }
};
var ST_KINDS = [
  ['cpu','CPU','целочисленная нагрузка на все ядра'], ['fpu','FPU','плавающая точка (AVX/FMA — самая горячая)'],
  ['cache','Кэш','рабочие наборы под L1/L2/L3'], ['memory','Память','запись/проверка паттернов в ОЗУ'],
  ['disk','Диск','цикл записи/чтения с проверкой (%TEMP%)'], ['gpu','GPU','тяжёлый шейдер WebGL']
];
var ST_UNITS = { cpu:'Мопс/с', fpu:'ГФлопс', cache:'МБ/с', memory:'МБ/с', disk:'МБ/с', gpu:'кадр/с' };
function fmtTime(sec){ var m=Math.floor(sec/60), r=sec%60; return String(m).padStart(2,'0')+':'+String(r).padStart(2,'0'); }

/* ----- GPU-нагрузка: тяжёлый фрагментный шейдер в WebGL (без зависимостей) ----- */
var gpuHost = null;
function gpuStressStart(){
  var st = S.st;
  try {
    if (!gpuHost){
      gpuHost = document.createElement('canvas');
      gpuHost.style.cssText = 'position:fixed;right:0;bottom:0;width:4px;height:4px;opacity:.02;pointer-events:none;z-index:1';
      document.body.appendChild(gpuHost);
    }
    gpuHost.width = 1600; gpuHost.height = 900;
    var gl = gpuHost.getContext('webgl');
    if (!gl){ st.events.push('GPU: WebGL недоступен в этом окне'); return; }
    function sh(type, src){ var o = gl.createShader(type); gl.shaderSource(o, src); gl.compileShader(o); return o; }
    var vs = sh(gl.VERTEX_SHADER, 'attribute vec2 a;void main(){gl_Position=vec4(a,0.,1.);}');
    var fs = sh(gl.FRAGMENT_SHADER, 'precision highp float;uniform vec2 r;uniform float t;void main(){vec2 p=(gl_FragCoord.xy/r-.5)*4.;float v=0.;for(int i=0;i<110;i++){p=abs(p)/dot(p,p)-vec2(.9+.1*sin(t),.7);v+=exp(-length(p));}gl_FragColor=vec4(vec3(v*.02),1.);}');
    var pr = gl.createProgram(); gl.attachShader(pr, vs); gl.attachShader(pr, fs); gl.linkProgram(pr); gl.useProgram(pr);
    var buf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 3,-1, -1,3]), gl.STATIC_DRAW);
    var loc = gl.getAttribLocation(pr, 'a'); gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    var ur = gl.getUniformLocation(pr, 'r'), ut = gl.getUniformLocation(pr, 't');
    gl.viewport(0, 0, 1600, 900); gl.uniform2f(ur, 1600, 900);
    var g = { frames:0, raf:0, timer:0, t0:performance.now(), on:true };
    function frame(){
      if (!g.on) return;
      gl.uniform1f(ut, (performance.now()-g.t0)/1000);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      g.frames++;
      g.raf = requestAnimationFrame(frame);
    }
    g.timer = setInterval(function(){ st.gpuFps = g.frames; g.frames = 0; }, 1000);
    st.gpu = g; frame();
  } catch(e){ st.events.push('GPU: не удалось запустить шейдер ('+(e && e.message ? e.message : e)+')'); }
}
function gpuStressStop(){
  var g = S.st.gpu; if (!g) return;
  g.on = false; cancelAnimationFrame(g.raf); clearInterval(g.timer);
  try { var gl = gpuHost && gpuHost.getContext('webgl'); if (gl){ var ext = gl.getExtension('WEBGL_lose_context'); if (ext) ext.loseContext(); } } catch(e){}
  if (gpuHost){ gpuHost.width = 1; gpuHost.height = 1; }
  S.st.gpu = null; S.st.gpuFps = null;
}

/* ----- обработка секундных данных ядра ----- */
function stExtra(p){
  var a = [];
  if (p.fanRpm!=null) a.push('вентилятор '+p.fanRpm.toFixed(0)+' об/мин');
  if (p.powerW!=null) a.push('CPU '+p.powerW.toFixed(0)+' Вт');
  return a.join(' · ');
}
function stOnTick(p){
  var st = S.st, h = st.hist;
  st.elapsed = p.elapsed; st.last = p;
  h.load.push(p.load); h.clock.push(p.clockMhz); h.clockMax = Math.max(h.clockMax, p.clockMaxMhz||0);
  h.temp.push(p.tempC==null ? null : p.tempC); h.gpuT.push(p.gpuTempC==null ? null : p.gpuTempC);
  Object.keys(p.scores||{}).forEach(function(k){ (h.scores[k] = h.scores[k] || []).push(p.scores[k]); });
  if (st.cfg.gpu && st.gpuFps>0) (h.scores.gpu = h.scores.gpu || []).push(st.gpuFps);
  (p.events||[]).forEach(function(e){ st.events.push(fmtTime(p.elapsed)+' · '+e); });
  paintStress();
  if (autoStressFocusActive()) render();   // экран «Идёт проверка» обновляется каждый тик
}
/* Вердикт по итогам: температура, падение скорости (троттлинг), ошибки данных, аварийная остановка. */
function judgeStress(res){
  var P = profile(), maxT = P.maxTempC || 95, minR = P.throttleMinPct!=null ? P.throttleMinPct : 60, maxShare = P.throttleMaxSharePct!=null ? P.throttleMaxSharePct : 20;
  var bad = [], notes = [];
  if (res.reason==='thermal') bad.push('сработала температурная защита');
  if (res.memErrors>0) bad.push('ошибки памяти: '+res.memErrors);
  if (res.diskErrors>0) bad.push('ошибки диска: '+res.diskErrors);
  var hot = Math.max(res.maxTempC||0, res.maxGpuTempC||0);
  if (hot>=maxT) bad.push('температура '+hot.toFixed(0)+' °C не ниже порога '+maxT+' °C');
  // Просадка «Мопс/с» и т.п. у cpu/fpu/cache сама по себе ненадёжна: на слабых
  // CPU без турбо (N95/N150, Ryzen U-серии) она стабильно ловилась на любом
  // ноутбуке из-за конкуренции с одновременной memory/disk/gpu-нагрузкой за
  // шину/кэш, а не из-за перегрева — частота при этом вообще не менялась
  // (реальные отчёты техников). Считаем троттлингом просадку скорости только
  // если она подтверждена реальным падением частоты CPU в этом же прогоне;
  // без данных о частоте (сенсор недоступен) — не считаем, а не гадаем.
  var freqDropped = res.clockAvgMhz > 0 && res.clockMinMhz > 0 && (res.clockMinMhz / res.clockAvgMhz) < 0.93;
  res.stressors.forEach(function(x){
    if (x.baseline>0 && freqDropped){
      var share = x.throttledSecs / Math.max(1, res.elapsedSecs) * 100;
      if (x.minRatio*100 < minR || share > maxShare) bad.push('падение скорости «'+x.name+'» до '+Math.round(x.minRatio*100)+'% от базовой ('+x.throttledSecs+' с ниже 80%), частота падала до '+res.clockMinMhz.toFixed(0)+' МГц (ср '+res.clockAvgMhz.toFixed(0)+')');
    }
  });
  var g = S.st.hist.scores.gpu;
  if (g && g.length>=12){
    var base = g.slice(2,12).sort(function(a,b){ return a-b; })[5], worst = Math.min.apply(null, g.slice(12));
    if (base>0 && worst/base*100 < minR) bad.push('падение скорости GPU до '+Math.round(worst/base*100)+'% от базовой');
  }
  if (res.reason==='stopped' && res.elapsedSecs<60) return null;   // слишком короткий прогон для оценки
  var base2 = res.threads+' потоков, '+fmtTime(res.elapsedSecs)+', загрузка '+res.avgLoad.toFixed(0)+'%';
  if (res.avgLoad<80 && res.stressors.length) notes.push('средняя загрузка CPU '+res.avgLoad.toFixed(0)+'% — нагрузка могла не дойти до предела');
  if (bad.length) return { status:'fail', note:base2+'; '+bad.join('; ') };
  return { status:'pass', note:base2+(hot?'; максимум '+hot.toFixed(0)+' °C':'; температура недоступна')+(res.clockAvgMhz?'; частота '+res.clockAvgMhz.toFixed(0)+' МГц':'')+(notes.length?'; '+notes.join('; '):'') };
}
function stOnDone(res){
  var st = S.st; st.running = false; st.res = res;
  var REASON = { completed:'завершён', stopped:'остановлен вручную', thermal:'остановлен температурной защитой', error:'остановлен из-за ошибок' };
  var lines = ['Тест '+(REASON[res.reason]||res.reason)+', длительность '+fmtTime(res.elapsedSecs)+', потоков CPU '+res.threads,
    'Загрузка CPU средняя '+res.avgLoad.toFixed(0)+'%'+(res.clockAvgMhz ? ', частота ср/мин/макс '+res.clockAvgMhz.toFixed(0)+'/'+res.clockMinMhz.toFixed(0)+'/'+res.clockMaxMhz.toFixed(0)+' МГц' : ''),
    'Температура CPU макс: '+(res.maxTempC!=null ? res.maxTempC.toFixed(0)+' °C' : 'н/д')+' · GPU макс: '+(res.maxGpuTempC!=null ? res.maxGpuTempC.toFixed(0)+' °C' : res.maxTempC!=null ? res.maxTempC.toFixed(0)+' °C (встроенная графика — в составе CPU, отдельного датчика нет)' : 'н/д')];
  res.stressors.forEach(function(x){ lines.push('  '+x.name+': ср '+x.avg.toFixed(1)+' '+x.unit+', мин '+x.min.toFixed(1)+', макс '+x.max.toFixed(1)+(x.baseline>0 ? ', базовая '+x.baseline.toFixed(1)+', худшее '+Math.round(x.minRatio*100)+'%, ниже 80%: '+x.throttledSecs+' с' : '')); });
  var gsc = st.hist.scores.gpu; if (gsc && gsc.length) lines.push('  gpu: ср '+(gsc.reduce(function(a,b){ return a+b; },0)/gsc.length).toFixed(1)+' кадр/с, мин '+Math.min.apply(null,gsc));
  if (res.memErrors||res.diskErrors) lines.push('Ошибки данных: память '+res.memErrors+', диск '+res.diskErrors);
  if (res.logFile) lines.push('Показания по секундам (CSV): '+res.logFile);
  st.events.forEach(function(e){ lines.push('! '+e); });
  recordDetail('stress', { lines:lines.slice(0,60), series: downsample(st.hist.load, 200) });
  var v = judgeStress(res);
  if (v) recordDetail('stress', { auto:v });
  render(); paintStress();
  if (S.auto.on && S.cat==='stress') A.autoApply(v);
}

/* ----- графики на canvas ----- */
function drawChart(id, series, opts){
  var cv = document.getElementById(id); if (!cv || !cv.getContext) return;
  var w = cv.clientWidth || 400, h = cv.clientHeight || 110;
  if (cv.width!==w) cv.width = w; if (cv.height!==h) cv.height = h;
  var g = cv.getContext('2d'); g.clearRect(0,0,w,h);
  var all = []; series.forEach(function(s){ s.data.forEach(function(v){ if (v!=null) all.push(v); }); });
  var min = opts.min!=null ? opts.min : (all.length ? Math.min.apply(null, all) : 0);
  var max = opts.max!=null ? opts.max : (all.length ? Math.max.apply(null, all) : 1);
  if (opts.line!=null) max = Math.max(max, opts.line);
  if (max<=min) max = min+1;
  var pad = (max-min)*0.08; min = opts.min!=null ? opts.min : min-pad; max += pad;
  g.strokeStyle='rgba(255,255,255,.07)'; g.lineWidth=1;
  for (var k=1;k<4;k++){ var y=h*k/4; g.beginPath(); g.moveTo(0,y); g.lineTo(w,y); g.stroke(); }
  var n = Math.max.apply(null, series.map(function(s){ return s.data.length; }).concat([2]));
  var span = opts.total && opts.total>n ? opts.total : n;
  series.forEach(function(s){
    g.strokeStyle = s.color; g.lineWidth = 1.6; g.beginPath(); var open = false;
    s.data.forEach(function(v,i){
      if (v==null){ open=false; return; }
      var x = i/(span-1)*w, y = h-(v-min)/(max-min)*h;
      if (!open){ g.moveTo(x,y); open=true; } else g.lineTo(x,y);
    });
    g.stroke();
  });
  if (opts.line!=null){ var yl = h-(opts.line-min)/(max-min)*h; g.strokeStyle='rgba(226,87,76,.7)'; g.setLineDash([5,4]); g.beginPath(); g.moveTo(0,yl); g.lineTo(w,yl); g.stroke(); g.setLineDash([]); }
  g.fillStyle='rgba(255,255,255,.5)'; g.font='10px JetBrains Mono, monospace';
  g.fillText(max.toFixed(0)+(opts.unit||''), 6, 12); g.fillText(min.toFixed(0), 6, h-4);
}
function paintStress(){
  var st = S.st, p = st.last, h = st.hist;
  var t = document.getElementById('st-time'); if (t) t.textContent = fmtTime(st.elapsed)+(st.cfg.dur ? ' / '+fmtTime(st.cfg.dur) : ' · до остановки');
  var f = document.getElementById('st-fill'); if (f) f.style.width = (st.cfg.dur ? Math.min(100, st.elapsed/st.cfg.dur*100) : (st.running?100:0))+'%';
  function set(id, v){ var e = document.getElementById(id); if (e) e.textContent = v; }
  if (p){
    set('st-load', p.load.toFixed(0)+' %');
    set('st-temp', (p.tempC!=null ? p.tempC.toFixed(0)+' °C' : '—')+(p.gpuTempC!=null ? ' · GPU '+p.gpuTempC.toFixed(0)+' °C' : ''));
    set('st-clock', p.clockMhz ? p.clockMhz.toFixed(0)+' МГц' : '—');
    set('st-clockmax', p.clockMaxMhz ? 'макс. '+p.clockMaxMhz.toFixed(0)+' МГц' : '');
    set('st-extra', stExtra(p));
  }
  var sc = document.getElementById('st-scores');
  if (sc){
    sc.innerHTML = Object.keys(h.scores).map(function(k){
      var arr = h.scores[k], cur = arr[arr.length-1], base = arr.length>=10 ? arr.slice(0,10).slice().sort(function(a,b){ return a-b; })[5] : 0;
      var ratio = base>0 ? cur/base : null;
      return '<div class="stscore"><span class="k">'+k+'</span><b>'+cur.toFixed(1)+'</b><span class="u">'+(ST_UNITS[k]||'')+'</span>'+(ratio!=null ? '<span class="r '+(ratio<0.8?'bad':'')+'">'+Math.round(ratio*100)+'% от базовой</span>' : '')+'</div>';
    }).join('');
  }
  var ev = document.getElementById('st-events');
  if (ev) ev.innerHTML = st.events.length ? st.events.slice(-30).map(function(e){ return '<div>'+esc(e)+'</div>'; }).join('') : '<div class="idle">событий пока нет</div>';
  var total = st.cfg.dur || 0;
  drawChart('st-c-load', [{ data:h.load, color:'#FF8A00' }], { min:0, max:100, unit:'%', total:total });
  drawChart('st-c-temp', [{ data:h.temp, color:'#FF8A00' }, { data:h.gpuT, color:'#6E8FA8' }], { unit:'°', line:profile().maxTempC||95, total:total });
  drawChart('st-c-clock', [{ data:h.clock, color:'#4CAF7D' }], { min:0, max:h.clockMax||null, unit:' МГц', total:total });
}
function verdictBar(c){
  return (S.markErr && S.markErr.id===c.id ? '<div class="markerr" id="mark-err">'+esc(S.markErr.text)+'</div>' : '')+
    '<div class="verdict">'+
      '<input placeholder="Комментарий техника — попадёт в отчёт" value="'+esc(S.comments[c.id]||'')+'" oninput="echips.comment(this.value)">'+
      '<button class="btn btn-ghost" onclick="echips.mark(\'na\')">Не применимо</button>'+
      '<button class="btn btn-danger" onclick="echips.mark(\'fail\')">Не пройден</button>'+
      '<button class="btn btn-primary" onclick="echips.mark(\'pass\')">Пройден</button>'+
    '</div>';
}
function markerBanner(){
  var m = S.stressMarker; if (!m) return '';
  return '<div class="markerr" style="margin:10px 0"><b>Прошлый стресс-тест был прерван</b> (перезагрузка, зависание или выключение питания) на '+m.lastElapsed+' с из '+(m.durationSecs||'«до остановки»')+
    '. Нагрузки: '+esc(m.stressors.join(', '))+'; последние показания: загрузка '+Math.round(m.lastLoad)+'%, температура '+(m.lastTempC!=null ? m.lastTempC.toFixed(0)+' °C' : 'н/д')+'.'+
    (m.logFile ? '<div class="kbnote" style="margin-top:6px">Показания по секундам сохранены: '+esc(m.logFile)+'</div>' : '')+
    '<div class="headactions" style="margin-top:8px;justify-content:flex-start">'+
    (m.durationSecs && m.durationSecs-m.lastElapsed>=60 ? '<button class="btn btn-primary" onclick="echips.markerResume()">Продолжить: осталось '+fmtTime(m.durationSecs-m.lastElapsed)+'</button>' : '')+
    '<button class="btn btn-danger" onclick="echips.markerRecord()">Записать в отчёт как «не пройден»</button>'+
    '<button class="btn btn-ghost" onclick="echips.markerDismiss()">Закрыть</button></div></div>';
}
function screenStress(){
  var st = S.st, c = st.cfg, res = st.res, run = st.running;
  var cat0 = CATS.filter(function(x){ return x.id==='stress'; })[0];
  var kinds = '<div class="stkinds">'+ ST_KINDS.map(function(k){
    return '<label class="stcheck'+(c[k[0]]?' on':'')+'" title="'+esc(k[2])+'"><input type="checkbox" '+(c[k[0]]?'checked':'')+' '+(run?'disabled':'')+' onchange="echips.stToggle(\''+k[0]+'\')"><span><b>'+k[1]+'</b><i>'+k[2]+'</i></span></label>';
  }).join('') +'</div>';
  var presets = '<div class="opts">'+ Object.keys(ST_PRESETS).map(function(k){
    return '<button class="opt mono" onclick="echips.stPreset(\''+k+'\')" '+(run?'disabled':'')+'>'+ST_PRESETS[k].label+'</button>';
  }).join('') +'</div>';
  var durs = [[60,'1 мин'],[300,'5 мин'],[600,'10 мин'],[1800,'30 мин'],[3600,'60 мин'],[0,'До остановки']];
  var opts = '<div class="stopts"><div class="control"><div class="k">Длительность</div><div class="opts">'+ durs.map(function(d){
      return '<button class="opt mono'+(c.dur===d[0]?' on':'')+'" onclick="echips.stSet(\'dur\','+d[0]+')" '+(run?'disabled':'')+'>'+d[1]+'</button>';
    }).join('') +'</div></div>'+
    '<div class="control"><div class="k">Потоков CPU</div><div class="opts">'+ [['all','Все'],['half','Половина'],['one','1']].map(function(d){
      return '<button class="opt mono'+(c.threads===d[0]?' on':'')+'" onclick="echips.stSet(\'threads\',\''+d[0]+'\')" '+(run?'disabled':'')+'>'+d[1]+'</button>';
    }).join('') +'</div></div>'+
    (c.memory ? '<div class="control"><div class="k">Память, % свободной</div><div class="opts">'+ [25,50,75].map(function(d){
      return '<button class="opt mono'+(c.memPct===d?' on':'')+'" onclick="echips.stSet(\'memPct\','+d+')" '+(run?'disabled':'')+'>'+d+'%</button>';
    }).join('') +'</div></div>' : '')+'</div>';
  var controls = '<div class="runrow" style="margin-top:12px"><button class="btn btn-primary" onclick="echips.stStart()" '+(run?'disabled':'')+'>'+(run?'Идёт нагрузка…':'Старт')+'</button>'+
    '<button class="btn btn-danger" onclick="echips.stStop()" '+(run?'':'disabled')+'>Стоп</button>'+
    '<button class="btn btn-ghost" onclick="echips.stClear()" '+(run?'disabled':'')+'>Очистить</button>'+
    '<span class="n" id="st-time" style="margin-left:6px;font-size:14px;color:var(--text)">'+fmtTime(st.elapsed)+(c.dur ? ' / '+fmtTime(c.dur) : ' · до остановки')+'</span></div>'+
    '<div class="bar" style="margin-top:10px"><div class="fill" id="st-fill" style="width:'+(c.dur ? Math.min(100, st.elapsed/c.dur*100) : (run?100:0))+'%"></div></div>';
  var p = st.last;
  var cards = '<div class="stats4" style="margin-top:12px">'+
    '<div class="stat4"><div class="k">загрузка CPU</div><div class="v" id="st-load">'+(p ? p.load.toFixed(0)+' %' : '—')+'</div></div>'+
    '<div class="stat4"><div class="k">температура</div><div class="v" id="st-temp" style="font-size:14px">'+(p ? (p.tempC!=null ? p.tempC.toFixed(0)+' °C' : '—')+(p.gpuTempC!=null ? ' · GPU '+p.gpuTempC.toFixed(0)+' °C' : '') : '—')+'</div><div class="k" id="st-extra" style="margin-top:2px">'+(p ? stExtra(p) : '')+'</div></div>'+
    '<div class="stat4"><div class="k">частота CPU</div><div class="v" id="st-clock" style="font-size:14px">'+(p && p.clockMhz ? p.clockMhz.toFixed(0)+' МГц' : '—')+'</div><div class="k" id="st-clockmax" style="margin-top:2px">'+(p && p.clockMaxMhz ? 'макс. '+p.clockMaxMhz.toFixed(0)+' МГц' : '')+'</div></div>'+
    '<div class="stat4"><div class="k">скорость нагрузок</div><div id="st-scores" class="stscores"></div></div></div>';
  var charts = '<div class="stcharts"><div><div class="k">Загрузка CPU, %</div><canvas id="st-c-load" class="stcanvas"></canvas></div>'+
    '<div><div class="k">Температура, °C <span style="color:#FF8A00">CPU</span> · <span style="color:#6E8FA8">GPU</span> · красная линия — порог защиты</div><canvas id="st-c-temp" class="stcanvas"></canvas></div>'+
    '<div><div class="k">Частота CPU, МГц</div><canvas id="st-c-clock" class="stcanvas"></canvas></div></div>';
  var evbox = '<div class="k" style="margin-top:12px">События</div><div class="log stev" id="st-events"><div class="idle">событий пока нет</div></div>';
  var summary = '';
  if (res){
    var v = judgeStress(res), REASON = { completed:'завершён', stopped:'остановлен вручную', thermal:'остановлен температурной защитой', error:'остановлен из-за ошибок' };
    summary = '<div class="kbnote" style="margin-top:10px">Тест '+(REASON[res.reason]||res.reason)+' · '+fmtTime(res.elapsedSecs)+
      (v ? ' · <b style="color:'+(v.status==='pass'?'var(--ok)':'var(--err)')+'">автооценка: '+(v.status==='pass'?'пройден':'не пройден')+'</b> — '+esc(v.note) : ' · оценка невозможна (слишком короткий прогон)')+'</div>';
  }
  return '<div class="pane">'+autoBanner()+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'dash\')">← все категории</button><span class="idx">стресс-тест · '+(run?'идёт':'готов')+'</span></div>'+
    '<div class="testhead"><div><h2>Стресс-тест</h2><div class="hint">Выберите виды нагрузки, как в AIDA64: CPU, FPU, кэш, память, диск, GPU. Скорость каждой нагрузки сравнивается с базовой — просадка означает троттлинг. Остановить можно в любой момент.</div></div><div class="base">STR · реальная нагрузка</div></div>'+
    '<div class="field">'+markerBanner()+
      '<div class="control"><div class="k">Пресеты</div>'+presets+'</div>'+
      '<div class="control" style="margin-top:10px"><div class="k">Виды нагрузки</div>'+kinds+'</div>'+opts+
      controls+(st.err ? '<div class="idle" style="color:var(--err);margin-top:8px"><span>'+esc(st.err)+'</span></div>' : '')+
      cards+charts+evbox+summary+
    '</div>'+ verdictBar(cat0) +'</div>';
}

function resultIcon(ok){
  var color = ok ? 'var(--ok)' : 'var(--err)';
  var inner = ok
    ? '<path d="M34 50l11 11 21-23" fill="none" stroke="'+color+'" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>'
    : '<path d="M40 40l20 20M60 40l-20 20" fill="none" stroke="'+color+'" stroke-width="6" stroke-linecap="round"/>';
  return '<div class="ic"><svg viewBox="0 0 100 100">'+
    '<polygon points="50,6 89,28 89,72 50,94 11,72 11,28" fill="none" stroke="'+color+'" stroke-width="4"/>'+inner+'</svg></div>';
}
function hexSpinner(label){
  return '<div class="scanpane"><div class="hex spin"><svg viewBox="0 0 100 100">'+
    '<polygon class="trk" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon>'+
    '<polygon class="arc" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon></svg></div>'+
    '<div class="status">'+label+'<span class="cur"></span></div></div>';
}

/* ---------- установка драйверов ---------- */
function categoryIcon(name){
  var n = name.toLowerCase(), st = ' fill="none" stroke="currentColor" stroke-width="1.6"';
  var icons = {
    net:'<circle cx="12" cy="12" r="1.6"/><path d="M5 15a10 10 0 0 1 14 0M8 11.5a6 6 0 0 1 8 0"'+st+'/>',
    media:'<path d="M6 10h3l4-3v10l-4-3H6z"/><path d="M16 9a4 4 0 0 1 0 6"'+st+'/>',
    bluetooth:'<path d="M8 7l8 6-5 4V3l5 4-8 6"'+st+' stroke-linejoin="round"/>',
    display:'<rect x="4" y="5" width="16" height="11" rx="1"'+st+'/><path d="M9 19h6M12 16v3" stroke="currentColor" stroke-width="1.6"/>',
    system:'<rect x="7" y="7" width="10" height="10" rx="1"'+st+'/><path d="M9 4v3M15 4v3M9 17v3M15 17v3M4 9h3M4 15h3M17 9h3M17 15h3" stroke="currentColor" stroke-width="1.4"/>',
    hidclass:'<circle cx="12" cy="9" r="2.4"'+st+'/><path d="M6 19c0-3 3-5 6-5s6 2 6 5"'+st+'/>',
    biometric:'<path d="M12 4a7 7 0 0 1 7 7c0 3-1 5-1 7M6 17c1-2 1-4 1-6a5 5 0 0 1 10 0c0 1 0 2-.3 3M9 20c1-2 1-4 1-6.2a2 2 0 0 1 4 0" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>',
    screaders:'<rect x="4" y="7" width="16" height="10" rx="1.5"'+st+'/><rect x="7" y="10" width="4" height="3" fill="currentColor"/>',
    usb:'<circle cx="12" cy="6" r="1.6"/><path d="M12 8v8M12 12h4a2 2 0 0 0 2-2V9M8 12v3a2 2 0 0 0 2 2h2"'+st+'/><circle cx="18" cy="9" r="1.4" fill="none" stroke="currentColor" stroke-width="1.4"/>',
    image:'<rect x="4" y="8" width="16" height="10" rx="1.5"'+st+'/><circle cx="12" cy="13" r="3"'+st+'/><path d="M9 8l1.5-2h3L15 8"'+st+'/>',
    audioprocessingobject:'<path d="M4 12h2l1.5-5 2 10 2-14 2 14 1.5-9 2 4h2" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>',
    softwarecomponent:'<rect x="5" y="5" width="7" height="7" rx="1"'+st+'/><rect x="12" y="12" width="7" height="7" rx="1"'+st+'/><path d="M12 8.5h4M8.5 12v4" stroke="currentColor" stroke-width="1.4"/>'
  };
  var body = '<rect x="5" y="5" width="14" height="14" rx="2"'+st+'/>';
  Object.keys(icons).some(function(k){ if(n.indexOf(k)>=0){ body=icons[k]; return true; } return false; });
  return '<svg viewBox="0 0 24 24" width="16" height="16">'+body+'</svg>';
}

/* Группирует модели по одинаковой ссылке на пакет: коды, использующие один
   пакет драйверов, показываются одной строкой ("Taganay NB156D / NB156D-H"). */
function buildModelGroups(manifest, autoKey){
  var keys = Object.keys(manifest).filter(function(k){ return k.charAt(0)!=='_' && manifest[k] && manifest[k].yandex_public_key; });
  var byLink = {}, order = [];
  keys.forEach(function(k){
    var l = manifest[k].yandex_public_key;
    if(!byLink[l]){ byLink[l]=[]; order.push(l); }
    byLink[l].push(k);
  });
  var groups = order.map(function(l){
    var ks = byLink[l];
    return { keys:ks, name:combineNames(ks, manifest), isAuto: autoKey ? ks.indexOf(autoKey)>=0 : false };
  });
  groups.sort(function(a,b){
    if(a.isAuto!==b.isAuto) return a.isAuto ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  return groups;
}
function combineNames(ks, manifest){
  function dn(k){ return manifest[k].display_name || k; }
  if(ks.length===1) return dn(ks[0]);
  var lines = ks.map(function(k){ var n=dn(k); return n.slice(-k.length)===k ? n.slice(0,n.length-k.length).trim() : null; });
  var same = lines[0] && lines.every(function(l){ return l===lines[0]; });
  return same ? lines[0]+' '+ks.join(' / ') : ks.map(dn).join(' / ');
}
function notifyDone(title, body){
  try {
    var n = window.__TAURI__.notification; if(!n) return;
    n.isPermissionGranted().then(function(ok){
      return ok ? 'granted' : n.requestPermission();
    }).then(function(p){ if(p==='granted') n.sendNotification({ title:title, body:body }); }).catch(function(){});
  } catch(e){}
}

function screenDrivers(){
  var d = S.drv, body;
  if(d.step==='idle' || d.step==='scan'){
    body = hexSpinner(d.scanLabel || 'СКАНИРОВАНИЕ ОБОРУДОВАНИЯ');
  } else if (d.step==='error'){
    body = '<div class="resultpane">'+resultIcon(false)+
      '<div class="msg">'+esc(d.error)+'</div>'+
      '<div class="actions"><button class="btn btn-ghost" onclick="echips.'+(d.backTo==='start'?"go('start')":"drvStep('"+(d.backTo||'pick')+"')")+'">Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.drvStart()">Начать заново</button></div></div>';
  } else if (d.step==='pick'){
    var auto = d.groups.filter(function(g){ return g.isAuto; })[0];
    body =
      '<div class="eyebrow">'+(d.autoKey?'Модель определена автоматически':'Модель не определена автоматически')+'</div>'+
      '<div class="n" style="font-family:var(--grotesk);font-size:19px;font-weight:600;color:var(--text-hi);margin:7px 0 14px">'+
      esc(auto ? auto.name : 'Выберите модель из списка')+'</div>'+
      '<input type="text" class="search-input" placeholder="Поиск модели..." oninput="echips.drvSearch(this.value)">'+
      '<div class="modellist" id="model-list">'+ d.groups.map(function(g,i){
        return '<label class="modelrow'+(g.isAuto?' rec':'')+'" data-search="'+esc((g.name+' '+g.keys.join(' ')).toLowerCase())+'">'+
          '<input type="radio" name="m" '+(i===d.pickIdx?'checked':'')+' onchange="echips.drvPick('+i+')">'+esc(g.name)+
          (g.isAuto?'<span class="tag">★ ОПРЕДЕЛЕНО АВТОМАТИЧЕСКИ</span>':'')+'</label>';
      }).join('') +'</div>'+
      '<div class="headactions" style="justify-content:space-between">'+
      '<button class="btn-link" onclick="echips.drvUniversal()">Не нашли модель? Универсальный набор →</button>'+
      '<button class="btn btn-primary" onclick="echips.drvSelect()">Выбрать</button></div>';
  } else if (d.step==='universal'){
    var n = Object.keys(d.uniPicked).length, hasProblems = (d.problems||[]).length>0;
    body =
      '<div class="eyebrow">Универсальный набор драйверов</div>'+
      '<div class="n" style="font-family:var(--grotesk);font-size:19px;font-weight:600;color:var(--text-hi);margin:7px 0 6px">Выберите категории для установки</div>'+
      '<div class="hint" style="margin-bottom:14px">'+(hasProblems
        ? 'Отмечены категории, соответствующие найденным проблемным устройствам — при желании выберите другие вручную.'
        : 'Явных ошибок не найдено — можно установить любые категории вручную.')+'</div>'+
      '<div class="catlist">'+ d.uniFiles.map(function(f,i){
        var on = !!d.uniPicked[i];
        return '<label class="catrow"><input type="checkbox" '+(on?'checked':'')+' onchange="echips.drvCat('+i+',this.checked)">'+
          '<span class="cat-icon">'+categoryIcon(f[0])+'</span>'+esc(f[0].replace(/\.zip$/i,''))+'</label>';
      }).join('') +'</div>'+
      '<div class="headactions" style="justify-content:space-between">'+
      '<button class="btn-link" onclick="echips.drvStep(\'pick\')">← Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.drvUniNext()">Далее</button></div>';
  } else if(d.step==='confirm'){
    var isModel = d.mode==='model', probs = d.problems||[];
    body =
      '<div class="card"><div class="k">'+(isModel?'Модель':'Универсальный набор')+'</div>'+
      '<div class="n">'+esc(isModel ? d.entry.name : 'Выбранные категории драйверов')+'</div>'+
      (isModel ? '<div class="s">Устройство: '+esc(deviceLabel())+' · SN '+esc(deviceSn())+'</div>'
               : '<div class="s">'+esc(d.chosenFiles.map(function(f){ return f[0].replace(/\.zip$/i,''); }).join(', '))+'</div>')+'</div>'+
      (probs.length
        ? '<div class="hint" style="margin:14px 0 8px">Найдено устройств без драйверов: '+probs.length+'</div>'+
          '<div class="devlist2">'+probs.map(function(x){
            return '<div class="devrow2"><span class="dot"></span>'+esc(x.friendly_name)+'<span class="cls">'+esc(x['class'])+'</span></div>';
          }).join('')+'</div>'
        : (isModel ? '<div class="hint" style="margin-top:14px">Явных ошибок с драйверами не найдено — но можно установить полный пакет драйверов для этой модели.</div>' : ''))+
      '<label class="checkrow"><input type="checkbox" '+(d.restore?'checked':'')+' onchange="echips.drvRestore(this.checked)">'+
      'Создать точку восстановления системы перед установкой</label>'+
      '<div class="headactions" style="margin-top:20px;justify-content:space-between">'+
      '<button class="btn-link" onclick="echips.drvStep(\''+(isModel?'pick':'universal')+'\')">← Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.drvInstall()">Скачать и установить</button></div>';
  } else if(d.step==='installing'){
    var p = d.progress || { pct:0, label:'', mb:'' };
    body =
      '<div class="card"><div class="k">Установка</div><div class="n" id="prog-label">'+esc(p.label)+'</div>'+
      '<div class="bar" style="margin-top:14px"><div class="fill" id="prog-fill" style="width:'+p.pct+'%"></div></div>'+
      '<div class="mbtext" id="prog-mb">'+esc(p.mb)+'</div>'+
      ((d.items||[]).length>1 ? '<div class="filecheck-list">'+d.items.map(function(name,i){
        var ok = !!d.doneIdx[i];
        return '<div class="filecheck-row'+(ok?' done':'')+'"><span class="filecheck-icon">'+(ok?'✓':'○')+'</span>'+esc(name)+'</div>';
      }).join('')+'</div>' : '')+'</div>';
  } else if(d.step==='done'){
    var res = d.result || { message:'Готово.', installed_drivers:[] };
    var inst = res.installed_drivers || [];
    body =
      '<div class="resultpane">'+resultIcon(true)+
      '<div class="msg">'+esc(res.message)+'</div>'+
      (inst.length ? '<button class="btn-link" onclick="echips.drvDetails()">'+(d.showDetails?'Скрыть':'Показать')+' детали установки ('+inst.length+')</button>'+
        (d.showDetails ? '<div class="installed-list">'+inst.map(function(x){ return '<div>'+esc(x)+'</div>'; }).join('')+'</div>' : '') : '')+
      '<div class="actions"><button class="btn btn-ghost" onclick="invoke_open_log()">Открыть лог</button>'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Позже</button>'+
      '<button class="btn btn-primary" onclick="echips_restart()">Перезагрузить сейчас</button></div></div>';
  }
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'start\')">← режимы</button>'+
    '<span class="idx">установка драйверов</span></div>'+
    '<div class="testhead"><div><h2>Установка драйверов</h2>'+
    '<div class="hint">Определение модели → выбор пакета (или универсальный набор) → установка с точкой восстановления.</div></div></div>'+
    '<div class="field" style="margin-top:16px">'+body+'</div></div>';
}
window.invoke_open_log = function(){ invoke('open_log_folder').catch(function(){}); };
window.echips_restart = function(){ invoke('restart_system').catch(function(){}); };

/* ---------- замена платы (гарантия) ---------- */
function screenMb(){
  var m = S.mb, body;
  if(m.step==='reading'){
    body = hexSpinner('ЧТЕНИЕ ТЕКУЩИХ SN/UUID С ПЛАТЫ');
  } else if(m.step==='readerror'){
    body =
      '<div class="resultpane">'+resultIcon(false)+
      '<div class="msg">'+esc(m.pinErr)+'</div>'+
      '<div class="actions"><button class="btn btn-ghost" onclick="echips.go(\'start\')">Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.mbReset()">Повторить</button></div></div>';
  } else if(m.step==='form'){
    body =
      '<div class="card"><div class="k">Текущие значения</div>'+
      '<div class="s" style="margin-top:8px">SN '+esc(m.before.serial_number)+'</div>'+
      '<div class="s">UUID '+esc(m.before.uuid)+'</div>'+
      '<div class="runrow" style="margin-top:10px"><button class="btn btn-ghost" onclick="echips.mbSaveIdentity()">Сохранить значения в файл</button>'+
      '<button class="btn btn-ghost" onclick="echips.mbChooseFile()">Выбрать файл и подставить…</button>'+
      '<input type="file" id="mb-file" accept=".json,.txt,.cfg,text/plain,application/json" style="display:none" onchange="echips.mbPickFile(this)">'+
      '<span class="kbnote">Перед заменой платы значения сохранятся в файл, папка откроется. После замены их можно подставить из файла.</span></div>'+
      (m.idMsg ? '<div class="kbnote" style="margin-top:8px;color:var(--ok)">'+esc(m.idMsg)+'</div>' : '')+
      (m.idErr ? '<div class="kbnote" style="margin-top:8px;color:var(--err)">'+esc(m.idErr)+'</div>' : '')+'</div>'+
      ((m.saved||[]).length ? '<div class="card" style="margin-top:12px"><div class="k">Сохранённые значения — подставить в форму</div><div class="smtable" style="margin-top:8px">'+m.saved.slice(0,8).map(function(x,i){
        return '<div class="smr" style="grid-template-columns:1.2fr 1.6fr 120px 70px 90px"><span class="mono">'+esc(x.serial||'—')+'</span><span class="mono">'+esc(x.uuid||'—')+'</span><span class="mono">'+esc(String(x.saved_at||'').slice(0,16).replace('T',' '))+'</span>'+
          '<button class="btn-link" onclick="echips.mbUseSaved('+i+')">подставить</button><button class="btn-link" onclick="echips.mbOpenFolder('+i+')">в папке</button></div>';
      }).join('')+'</div></div>' : '')+
      '<div class="formgrid" style="margin-top:16px">'+
      '<div class="formfield"><label>Номер наряда</label><input value="'+esc(m.ticket)+'" oninput="echips.mbField(\'ticket\',this.value)" placeholder="Гарантийный случай / наряд"></div>'+
      '<div class="formfield"><label>Новый серийный номер</label><input value="'+esc(m.serial)+'" oninput="echips.mbField(\'serial\',this.value)" placeholder="4–40 символов; пусто — не менять"></div>'+
      '<div class="formfield"><label>Новый UUID</label><input value="'+esc(m.uuid)+'" oninput="echips.mbField(\'uuid\',this.value)" placeholder="8-4-4-4-12; пусто — не менять"></div>'+
      (m.formErr?'<div class="err" style="margin:-6px 0 12px">'+esc(m.formErr)+'</div>':'')+
      '</div>'+
      '<div class="headactions">'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.mbNext()">Далее</button></div>';
  } else if(m.step==='confirm'){
    body =
      '<div class="kvgrid">'+
      '<div class="h">Поле</div><div class="h">Было</div><div class="h">Будет</div>'+
      '<div class="lbl">SN</div><div class="old">'+esc(m.before.serial_number)+'</div><div class="new">'+(m.serial ? esc(m.serial) : '<span style="opacity:.6">не меняется</span>')+'</div>'+
      '<div class="lbl">UUID</div><div class="old">'+esc(m.before.uuid)+'</div><div class="new">'+(m.uuid ? esc(m.uuid) : '<span style="opacity:.6">не меняется</span>')+'</div>'+
      '</div>'+
      '<div class="s" style="margin-top:14px">Наряд: '+esc(m.ticket)+' · Техник: '+esc(m.techName)+'</div>'+
      '<div class="warnbox">Запись необратимо меняет SN/UUID платы (утилита завода: AMI — AMIDEWIN, Insyde — H2OSDE). '+
      'После записи серийник читается обратно и сверяется; в Windows новые значения видны после перезагрузки. Попытка попадёт в аудит-лог с хэш-цепочкой.</div>'+
      '<div class="headactions" style="margin-top:16px">'+
      '<button class="btn btn-ghost" onclick="echips.mbBack()">Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.mbWrite()">Записать</button></div>';
  } else if(m.step==='writing'){
    body = hexSpinner('ЗАПИСЬ SN/UUID');
  } else if(m.step==='stub'){
    body =
      '<div class="resultpane">'+resultIcon(false)+
      '<div class="msg">'+esc(m.writeError)+'</div>'+
      '<div class="actions"><button class="btn btn-primary" onclick="echips.go(\'start\')">Понятно</button></div></div>';
  } else {
    body =
      '<div class="resultpane">'+resultIcon(true)+
      '<div class="msg">'+(m.serial&&m.uuid ? 'SN и UUID' : m.serial ? 'SN' : 'UUID')+' записан'+(m.serial&&m.uuid ? 'ы' : '')+' и подтвержд'+(m.serial&&m.uuid ? 'ены' : 'ён')+' чтением обратно. Перезагрузите ПК, чтобы Windows показал новые значения. Запись сохранена в журнал аудита.</div>'+
      '<div class="actions"><button class="btn btn-ghost" onclick="echips.mbVerify()">Проверить идентификаторы</button><button class="btn btn-primary" onclick="echips.go(\'start\')">Готово</button></div></div>';
  }
  var stepIdx = { reading:0, readerror:0, form:1, confirm:2, writing:3, stub:3 }[m.step]; if (stepIdx===undefined) stepIdx = 4;
  var steps = ['Чтение с платы','Новые значения','Проверка','Запись','Готово'].map(function(t,i){
    return '<div class="mb-st'+(i<stepIdx?' done':i===stepIdx?' on':'')+'"><i>'+(i<stepIdx?'✓':(i+1))+'</i><span>'+t+'</span></div>';
  }).join('');
  return '<div class="pane te mbx">'+
    '<div class="af-head"><div><div class="eyebrow">Замена платы · гарантия</div><h1 class="title">Запись SN и UUID</h1></div>'+
    '<button class="btn btn-ghost" onclick="echips.go(\'start\')">← К режимам</button></div>'+
    '<div class="mb-steps">'+steps+'</div>'+
    '<div class="hint mb-hint">Доступ только для авторизованного техника. Чтение SN/UUID — WMI; запись — заводской утилитой (AMI/Insyde) с проверкой чтением обратно.</div>'+
    '<section class="af-pan mb-body">'+body+'</section></div>';
}

/* ---------- добавление инженера ----------
   Список инженеров живёт в _config/techs.json (приватный echips-reports) публичного репозитория (см.
   commands/techs.rs) — этот экран не пишет туда напрямую (для этого
   понадобился бы токен на запись в репозиторий, у приложения его нет и не
   должно быть), а только считает PIN так же, как это потом сделает вход
   (sha256Hex(salt+":"+pin)), и показывает готовый JSON-блок для вставки в
   файл вручную — commit/push уже делает тот, кто добавляет инженера. */
function screenTechAdmin(){
  var t = S.techadmin;
  var rows = (S.lock.techs||[]).map(function(x){
    return '<tr><td><i class="av">'+esc(String(x.name||'?').charAt(0).toUpperCase())+'</i></td><td><b>'+esc(x.name)+'</b><div class="rr mono">'+esc(x.id)+'</div></td>'+
      '<td><span class="role'+(x.role==='admin'?' adm':'')+'">'+(x.role==='admin'?'Администратор':'Техник')+'</span></td>'+
      '<td class="ra"><button class="lnk bad" '+(t.busy?'disabled':'')+' onclick="echips.techadminRemove(\''+esc(x.id)+'\')">Удалить</button></td></tr>';
  }).join('');
  return '<div class="pane te">'+
    '<div class="af-head"><div><div class="eyebrow">Только для администратора</div><h1 class="title">Инженеры</h1></div><button class="btn btn-ghost" onclick="echips.go(\'start\')">Закрыть</button></div>'+
    '<div class="rp-wrap"><section>'+
      (rows ? '<table class="rp-tb te-tb"><thead><tr><th></th><th>Имя</th><th>Роль</th><th></th></tr></thead><tbody>'+rows+'</tbody></table>' : '<p class="rs-mut">Список пуст или не загружен.</p>')+
      '<div class="te-form"><h3>Новый инженер или смена PIN</h3><div class="te-frm">'+
        '<label>Идентификатор латиницей<input value="'+esc(t.id)+'" oninput="echips.techadminField(\'id\',this.value)" placeholder="sidorov"></label>'+
        '<label>ФИО<input value="'+esc(t.name)+'" oninput="echips.techadminField(\'name\',this.value)" placeholder="Сидоров С.С."></label>'+
        '<label>PIN, от 4 цифр<input type="password" value="'+esc(t.pin)+'" oninput="echips.techadminField(\'pin\',this.value)" placeholder="••••••"></label>'+
        '<label>Роль<select onchange="echips.techadminField(\'role\',this.value)"><option value="tech"'+(t.role!=='admin'?' selected':'')+'>Техник</option><option value="admin"'+(t.role==='admin'?' selected':'')+'>Администратор</option></select></label></div>'+
        (t.err?'<div class="err" style="margin:10px 0 0">'+esc(t.err)+'</div>':'')+
        (t.msg?'<div class="infoline" style="margin:10px 0 0">'+esc(t.msg)+'</div>':'')+
        '<div class="headactions" style="margin-top:14px"><button class="btn btn-primary" '+(t.busy?'disabled':'')+' onclick="echips.techadminPublish()">'+(t.busy?'Сохраняю…':'Сохранить в список')+'</button>'+
        '</div>'+
        '<p class="rs-mut" style="margin-top:12px">Инженеры хранятся на сервере: изменения действуют сразу на всех ноутбуках. Тот же идентификатор с новым PIN заменяет запись. Все входы и неудачные попытки попадают в журнал сервера.</p></div>'+
    '</section><aside class="rp-side"><h3>Как это работает</h3><p class="rs-mut">PIN проверяется на сервере, на ноутбуке он не хранится. Сессия действует 12 часов. Пять неверных PIN подряд блокируют вход с этого адреса на 15 минут.</p></aside></div></div>';
}

function screenReport(){
  var c = counts(), tests = profile().tests||[], full = tests.every(function(id){ return statusOf(id)!=='idle'; });
  var verdict = c.fail ? 'Нужен ремонт' : full ? 'Годен' : 'Проверка не завершена';
  var cls = c.fail ? 'fail' : full ? 'pass' : 'idle';
  var na = 0, idle = tests.filter(function(id){ return statusOf(id)==='idle'; }).length;
  CATS.forEach(function(x){ if (statusOf(x.id)==='na') na++; });
  var failed = CATS.filter(function(x){ return statusOf(x.id)==='fail'; });
  var sub = failed.length ? failed.map(function(x){ return x.name+(dashNote(x.id)?': '+dashNote(x.id):''); }).slice(0,2).join('. ')+'.' : (full ? 'Все проверки профиля выполнены, ошибок нет.' : 'Часть проверок профиля ещё не выполнена.');
  var send = S.reportSend==='sent' ? '<p class="rs-ok">Отчёт отправлен администратору.</p>' : S.reportSend==='queued' ? '<p class="rs-q">Отчёт в очереди: уйдёт при появлении связи.</p>' : '<p class="rs-mut">Отчёт уйдёт после автопрогона или экспорта.</p>';
  return '<div class="pane rp">'+
    '<div class="rp-head"><div class="rp-v '+cls+'"><p class="rp-k">Итог проверки</p><h1>'+verdict+'</h1><p class="rp-sub">'+esc(sub)+'</p></div>'+
      '<div class="rp-c"><div class="dst pass"><b>'+c.pass+'</b><span>пройдено</span></div><div class="dst fail"><b>'+c.fail+'</b><span>ошибка</span></div><div class="dst na"><b>'+na+'</b><span>не применимо</span></div><div class="dst idle"><b>'+idle+'</b><span>не проверено</span></div></div>'+
      '<div class="headactions"><button class="btn btn-ghost" onclick="echips.exp(\'txt\')">TXT</button><button class="btn btn-ghost" onclick="echips.exp(\'json\')">JSON</button><button class="btn btn-primary" onclick="echips.exp(\'pdf\')">Сохранить PDF</button></div></div>'+
    '<div class="rp-wrap"><table class="rp-tb"><thead><tr><th></th><th>Проверка</th><th>Статус</th><th>Результат и комментарий</th></tr></thead><tbody>'+
      CATS.map(function(x){
        var st = statusOf(x.id), out = st==='idle' && !inProfile(x.id);
        var cm = S.comments[x.id] || (x.id==='sens' && S.snapshot ? 'приложен снимок датчиков' : dashNote(x.id) || '');
        return '<tr class="'+st+' clickable" onclick="echips.repOpen(\''+x.id+'\')" title="Открыть подробности"><td><i class="ddot '+st+'"></i></td><td class="rn">'+x.name+'</td><td class="rl">'+(out?'вне профиля':STATUS[st].label)+'</td><td class="rr">'+esc(cm)+'</td></tr>';
      }).join('')+'</tbody></table>'+
    '<aside class="rp-side"><h3>Заключение инженера</h3><textarea class="repsummary" rows="6" placeholder="Итог по устройству, что сделано, на что обратить внимание клиенту или сервису…" oninput="echips.reportSummary(this.value)">'+esc(S.reportSummary||'')+'</textarea>'+
      '<h3>Отправка</h3>'+send+
      (S.exported ? '<p class="rs-mut">'+esc(S.exported.path)+' сохранён</p>' : '<p class="rs-mut">TXT для акта, JSON для базы, PDF для клиента.</p>')+
      '<h3>Устройство</h3><p class="rs-dev">'+esc(deviceLabel())+'<br><span class="mono">SN '+esc(deviceSn())+'</span></p></aside></div></div>';
}

/* ---------- рендер ---------- */
/* ---------- подробности теста из отчёта ---------- */
function screenRepDetail(){
  var id = S.repId, c = null;
  CATS.forEach(function(x){ if (x.id===id) c = x; });
  if (!c) return screenReport();
  var d = S.detail[id] || {}, st = statusOf(id), out = st==='idle' && !inProfile(id);
  var a = d.auto, meta = [];
  if (d.ts) meta.push('время: '+new Date(d.ts).toLocaleString('ru-RU'));
  if (a && a.status) meta.push('автооценка: '+({pass:'пройден',fail:'не пройден',na:'не применимо'}[a.status]||a.status)+' — '+esc(a.note));
  if (d.override) meta.push('<span style="color:var(--err)">вердикт изменён техником ('+({pass:'пройден',fail:'не пройден',na:'не применимо'}[d.override.to]||d.override.to)+'): '+esc(d.override.reason)+'</span>');
  var lines = (d.lines||[]);
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'report\')">← к отчёту</button><span class="idx">подробности теста</span></div>'+
    '<div class="testhead"><div><h2>'+esc(c.name)+'</h2><div class="hint">'+esc(c.method)+'</div></div>'+
    '<div class="base"><span class="pill '+STATUS[st].cls+'"><i></i>'+(out?'вне профиля':STATUS[st].label)+'</span></div></div>'+
    '<div class="field">'+
      (S.comments[id] ? '<div class="kbnote" style="margin-bottom:10px;color:var(--text-2)">Комментарий: '+esc(S.comments[id])+'</div>' : '')+
      (meta.length ? '<div class="kbnote" style="margin-bottom:10px">'+meta.join('<br>')+'</div>' : '')+
      (d.series && d.series.length ? sparkBars(d.series)+'<div class="kbnote" style="margin-top:6px">график скорости по ходу теста, МБ/с</div>' : '')+
      (lines.length ? '<div class="log" style="margin-top:12px">'+lines.map(function(t,i){ return '<div><span class="t">'+String(i+1).padStart(2,'0')+'</span><span style="white-space:pre-wrap">'+esc(t)+'</span></div>'; }).join('')+'</div>'
        : '<div class="idle" style="margin-top:8px"><span class="t">--</span><span>'+(out?'Тест не входит в профиль этой модели и в прогоне не участвовал.':'Подробных данных нет — тест выполнялся вручную, результат и комментарий указаны выше.')+'</span></div>')+
    '</div>'+
    '<div class="verdict"><button class="btn btn-ghost" onclick="echips.go(\'report\')">К отчёту</button>'+
      '<button class="btn btn-primary" onclick="echips.openCat(\''+id+'\')">Открыть тест</button></div></div>';
}

function render(){
  renderNav();
  var host = document.getElementById('screen');
  var focus = document.activeElement, sel = null;
  var focusKey = null;
  if(focus && (focus.tagName==='INPUT' || focus.tagName==='TEXTAREA') && focus.type!=='file'){ sel = focus.selectionStart; focusKey = focus.id || focus.getAttribute('placeholder') || ''; }
  var viewKey = S.screen+'|'+S.cat+'|'+(S.screen==='drivers'?S.drv.step:'')+'|'+(S.screen==='mb'?S.mb.step:'');
  var isNewView = viewKey !== S.viewKey; S.viewKey = viewKey;
  host.innerHTML = S.screen==='start' ? screenStart()
    : S.screen==='drivers' ? screenDrivers()
    : S.screen==='mb' ? screenMb()
    : S.screen==='techadmin' ? screenTechAdmin()
    : S.screen==='dash' ? screenDash()
    : S.screen==='test' ? screenTest()
    : S.screen==='repdetail' ? screenRepDetail()
    : S.screen==='sensors' ? (autoSensorsFocusActive() ? screenAutoFocusSensors() : screenSensors())
    : S.screen==='stress' ? (autoStressFocusActive() ? screenAutoFocusStress() : screenStress())
    : S.screen==='events' ? screenEvents()
    : S.screen==='history' ? screenHistory() : screenReport();
  if (isNewView && host.firstElementChild) host.firstElementChild.classList.add('enter');
  if(sel!==null){
    var inp = null;
    host.querySelectorAll('input,textarea').forEach(function(x){ if (!inp && (x.id || x.getAttribute('placeholder') || '') === focusKey) inp = x; });
    if(!inp) inp = host.querySelector('input');
    if(inp){ inp.focus(); try{ inp.setSelectionRange(sel,sel); }catch(e){} }
  }
  if (S.screen==='test' && cat().kind==='surface') paintSurface();
  if (S.screen==='test' && cat().kind==='touch') bindTouchArea();
  if (S.screen==='stress') paintStress();
  var pad = document.getElementById('pad');
  if(pad){
    pad.addEventListener('pointerdown', padPoint);
    pad.addEventListener('pointermove', function(e){ if(e.buttons) padPoint(e,true); });
  }
  var video = document.getElementById('cam-preview');
  if (video && S.camStream && video.srcObject !== S.camStream) video.srcObject = S.camStream;
}
function padPoint(e, move){
  var r = e.currentTarget.getBoundingClientRect();
  S.padDots = S.padDots.concat([{ x:((e.clientX-r.left)/r.width*100).toFixed(1), y:((e.clientY-r.top)/r.height*100).toFixed(1) }]).slice(-140);
  if(move) S.padMoves++; else { S.padCount++; S.padMax = Math.max(S.padMax,1); }
  render();
}

/* ---------- аудио-спектр: перерисовка на кадр, пока играет тон ---------- */
(function audioLoop(){
  requestAnimationFrame(audioLoop);
  if (S.tone!==null && S.screen==='test' && (cat().kind==='audio' || cat().kind==='headset')){
    // Обновляем только спектр: полная перерисовка каждый кадр ломала клики по кнопкам.
    var sp = document.querySelector('.spectrum');
    if (sp) sp.innerHTML = spectrumBars();
  }
})();

/* ---------- вход по PIN при запуске ----------
   PIN проверяет сервер Echips (commands/srv.rs). Экран поверх всего приложения (#lock-overlay в
   index.html, вне #screen — render() его не трогает). */
function lockInit(){
  S.lock = { phase:'pin', techs:null, err:'', pin:'', shake:false, note:'' };
  renderLock();
  // связи с сервером нет — предупреждаем сразу, а не после ввода PIN
  invoke('srv_ping').then(function(ok){
    if (!ok){ S.lock.note = 'Нет связи с сервером Echips. Вход без интернета возможен, если вы входили на этом ноутбуке с интернетом за последние 7 суток.'; renderLock(); }
  }).catch(function(){});
}
/* Вход: PIN уходит на сервер (commands/srv.rs), он же выдаёт сессию на 12 часов и ведёт журнал входов. */
function lockTrySubmit(){
  var L = S.lock;
  if(!L.pin){ L.err='Введите PIN.'; renderLock(); return; }
  L.phase='verifying'; renderLock();
  invoke('srv_login', { pin:L.pin }).then(function(res){
    if (res && res.ok){
      S.engineer = { id:res.id, name:res.name, role:res.role||'tech', offline:!!res.offline_mode };
      L.note = ''; L.phase='ok'; renderLock(); render();
      afterLogin();
      setTimeout(function(){ L.phase='unlocked'; renderLock(); }, 650);
    } else {
      L.phase='pin'; L.pin='';
      L.err = res && res.offline ? (res.message || 'Нет связи с сервером. Проверьте интернет и повторите.') : ((res && res.message) || 'Неверный PIN.');
      L.shake=true; renderLock();
      setTimeout(function(){ L.shake=false; renderLock(); }, 400);
    }
  }).catch(function(err){
    L.phase='pin'; L.pin=''; L.err = typeof err==='string' ? err : 'Не удалось войти'; renderLock();
  });
}
/* После входа: профили моделей, очередь неотправленных отчётов, статус. */
function afterLogin(){
  loadProfiles();
  invoke('flush_report_queue').catch(function(){}).then(refreshQueue);
}
/* Сессия на сервере истекла (12 ч) — снова показываем экран PIN, результаты проверки остаются. */
function relockSession(){
  if (!FEATURE_PIN || S.lock.phase!=='unlocked') return;
  S.lock.phase='pin'; S.lock.pin=''; S.lock.err='Сессия истекла — введите PIN снова, результаты проверки сохранены.';
  renderLock();
}
function dotsHtml(n){
  var out = '';
  for (var i=0;i<Math.max(n,4);i++) out += '<span class="lock-dot'+(i<n?' on':'')+'"></span>';
  return out;
}
function keypadHtml(){
  var keys = ['1','2','3','4','5','6','7','8','9','','0','⌫'];
  return keys.map(function(k){
    if(k==='') return '<span class="lock-key" style="visibility:hidden"></span>';
    if(k==='⌫') return '<button type="button" class="lock-key" onclick="echips.lockBackspace()">⌫</button>';
    return '<button type="button" class="lock-key" onclick="echips.lockDigit(\''+k+'\')">'+k+'</button>';
  }).join('') + '<button type="button" class="lock-key ok wide" style="grid-column:1/4" onclick="echips.lockSubmit()">Войти</button>';
}
function renderLock(){
  var host = document.getElementById('lock-overlay');
  if(!host) return;
  var L = S.lock, body;
  document.body.classList.toggle('locked', L.phase!=='unlocked');   // баннер обновления поверх экрана PIN
  if(L.phase==='unlocked'){
    host.classList.add('closed');
    setTimeout(function(){ if(S.lock.phase==='unlocked') host.innerHTML=''; }, 400);
    return;
  }
  host.classList.remove('closed');
  if(L.phase==='boot'){
    body = '<div class="lock-spin"><svg viewBox="0 0 100 100">'+
      '<polygon class="trk" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon>'+
      '<polygon class="arc" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon></svg></div>'+
      '<div class="lock-status">Загрузка списка инженеров…</div>';
  } else if(L.phase==='error'){
    body = '<div class="lock-status err">'+esc(L.err)+'</div>'+
      '<button type="button" class="btn btn-primary" style="margin-top:16px" onclick="echips.lockRetry()">Повторить</button>';
  } else if(L.phase==='ok'){
    body = '<div class="lock-ok-check"><svg viewBox="0 0 52 52">'+
      '<circle cx="26" cy="26" r="23"></circle><path d="M15 27l7 7 15-15"></path></svg></div>'+
      '<div class="lock-status">Добро пожаловать, '+esc(S.engineer?S.engineer.name:'')+'</div>';
  } else {
    body =
      '<div class="lock-dots'+(L.shake?' shake':'')+'" style="margin-top:8px">'+dotsHtml(L.pin.length)+'</div>'+
      '<div class="lock-keypad">'+keypadHtml()+'</div>'+
      (L.err?'<div class="lock-err">'+esc(L.err)+'</div>':'')+
      (L.note?'<div class="lock-err" style="color:#F0C24B;max-width:300px;margin-top:10px">'+esc(L.note)+' <button type="button" class="btn-link" onclick="echips.lockRetry()">Обновить</button></div>':'');
  }
  host.innerHTML = '<div class="lock-card">'+
    '<div class="lock-logo"><img src="logo.png" alt="Echips"></div>'+
    '<div class="lock-title">Echips Hardware Check</div>'+
    body+
  '</div>';
}
/* Физическая клавиатура для ввода PIN — работает, только пока открыт
   экран входа (phase 'pin'), чтобы не конфликтовать со слушателем теста
   клавиатуры (тот включён лишь на S.screen==='test' с категорией kb). */
document.addEventListener('keydown', function(e){
  if (!FEATURE_PIN || S.lock.phase!=='pin') return;
  if (/^[0-9]$/.test(e.key)){ e.preventDefault(); A.lockDigit(e.key); }
  else if (e.key==='Backspace'){ e.preventDefault(); A.lockBackspace(); }
  else if (e.key==='Enter'){ e.preventDefault(); A.lockSubmit(); }
});

/* ---------- админ-панель (Shift+F10) ----------
   Идея пользователя: список всех IPC-вызовов (invoke → Rust) с результатом,
   для диагностики на месте. Доступно только role==='admin' (сейчас — только
   аккаунт Максима, см. isAdmin/_config/techs.json (приватный echips-reports)) и не во время теста
   клавиатуры (там F10 — часть проверяемой раскладки, ловить его нельзя). */
function renderAdminPanel(){
  var host = document.getElementById('admin-panel');
  if(!host) return;
  if(!S.adminPanelOpen){ host.innerHTML=''; return; }
  var rows = S.adminLog.slice().reverse().map(function(e){
    var time = new Date(e.t).toLocaleTimeString();
    var cls = e.state==='err' ? 'err' : e.state==='ok' ? 'ok' : 'pending';
    var tail = e.state==='err' ? esc(typeof e.error==='string'?e.error:JSON.stringify(e.error))
      : e.state==='ok' ? esc(JSON.stringify(e.result)).slice(0,300)
      : '…';
    return '<div class="admin-row '+cls+'"><span class="t">'+time+'</span>'+
      '<span class="cmd">'+esc(e.cmd)+'</span>'+
      '<span class="args">'+esc(JSON.stringify(e.args||{})).slice(0,200)+'</span>'+
      '<span class="res">'+tail+'</span></div>';
  }).join('');
  host.innerHTML =
    '<div class="admin-head">Вывод команд (Shift+F10) — '+S.adminLog.length+'<button type="button" class="btn-link" onclick="echips.adminClose()">Закрыть ×</button></div>'+
    '<div class="admin-rows">'+(rows || '<div class="admin-empty">Пока нет вызовов</div>')+'</div>';
}
document.addEventListener('keydown', function(e){
  if (e.key!=='F10' || !e.shiftKey) return;
  if (S.screen==='test' && cat().kind==='keyboard') return; // F10 — часть проверяемой раскладки
  if (!isAdmin()) return;
  e.preventDefault();
  S.adminPanelOpen = !S.adminPanelOpen;
  renderAdminPanel();
});

document.addEventListener('DOMContentLoaded', function(){
  loadDevice();
  loadProfiles();
  render();
  if (FEATURE_PIN) {
    lockInit();
  } else {
    // автовход под Максимом: без экрана PIN и без сети
    S.engineer = { id:'maksim', name:'Максим', role:'admin' };
    S.lock.phase = 'unlocked';
    var ov = document.getElementById('lock-overlay');
    if (ov && ov.parentNode) ov.parentNode.removeChild(ov);
    render();
  }
  invoke('flush_report_queue').catch(function(){}).then(refreshQueue);
  // сессия на сервере живёт 12 часов: когда истечёт — просим PIN заново
  setInterval(function(){
    if (FEATURE_PIN && S.lock.phase==='unlocked') invoke('srv_whoami').then(function(w){
      if (!w){ relockSession(); return; }
      // связь появилась (или пропала) — обновляем метку «без сети»
      if (S.engineer && !!S.engineer.offline !== !!w.offline){ S.engineer.offline = !!w.offline; renderNav(); refreshQueue(); }
    }).catch(function(){});
  }, 60000);
  setInterval(function(){ if (!S.auto.on) A.reportSync('sync'); refreshQueue(); }, 10000);
  // Отчёты, накопленные без сети, досылаем сами: раз в 3 минуты и сразу при появлении связи.
  setInterval(function(){ invoke('flush_report_queue').catch(function(){}); }, 60000);
  window.addEventListener('online', function(){ invoke('flush_report_queue').catch(function(){}); });
});
})();
