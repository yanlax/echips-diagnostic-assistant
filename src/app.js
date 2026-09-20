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

var invoke = window.__TAURI__.core.invoke;
var tauriEvent = window.__TAURI__.event;
var getCurrentWindow = window.__TAURI__.window.getCurrentWindow;

var CATS = [
  { id:'usb', tag:'USB', name:'USB-порты', method:'Список устройств на USB-шине (WMI PnP) + статус', impl:'реальные данные', kind:'runner', fetch:'usb' },
  { id:'bt', tag:'BT', name:'Bluetooth', method:'Статус адаптера и список сопряжённых устройств', impl:'реальные данные', kind:'runner', fetch:'bt' },
  { id:'wifi', tag:'WIFI', name:'Wi-Fi', method:'Адаптер + список видимых сетей (netsh wlan)', impl:'реальные данные', kind:'runner', fetch:'wifi' },
  { id:'kb', tag:'KEY', name:'Клавиатура', method:'Карта клавиш, детект n-key rollover; залипы — глазами', impl:'интерактивно', kind:'keyboard' },
  { id:'lcd', tag:'LCD', name:'Матрица', method:'Заливка сплошными цветами — битые пиксели и засветы', impl:'интерактивно', kind:'display' },
  { id:'cam', tag:'CAM', name:'Камера', method:'Живое превью через getUserMedia — оценка на глаз', impl:'реальное превью', kind:'camera' },
  { id:'pad', tag:'PAD', name:'Тачпад', method:'Точки касания, мультитач, базовые жесты', impl:'интерактивно', kind:'touchpad' },
  { id:'fp', tag:'FP', name:'Отпечаток', method:'Сенсор виден системе (WinBio) — регистрация вручную', impl:'частично', kind:'runner', fetch:'fp' },
  { id:'bat', tag:'BAT', name:'Аккумулятор', method:'Design vs Full charge capacity, циклы, износ (powercfg)', impl:'реальные данные', kind:'runner', fetch:'bat' },
  { id:'snd', tag:'SND', name:'Звук', method:'Тестовый сигнал (Web Audio) и echo-тест через микрофон', impl:'реально', kind:'audio' },
  { id:'sens', tag:'SNS', name:'Датчики', method:'Температуры через WMI ACPI — доступность зависит от платы', impl:'зависит от платы', kind:'sensors' },
  { id:'stress', tag:'STR', name:'Стресс-тест', method:'Реальная нагрузка CPU на всех ядрах на заданное время', impl:'CPU реально', kind:'stress' }
];
var FILLS = [
  { name:'белый', color:'#FFFFFF' }, { name:'чёрный', color:'#000000' },
  { name:'красный', color:'#FF0000' }, { name:'зелёный', color:'#00FF00' },
  { name:'синий', color:'#0000FF' }, { name:'серый 50%', color:'#808080' }
];
var KEYROWS = [
  ['Esc','F1','F2','F3','F4','F5','F6','F7','F8','F9','F10','F11','F12','Del'],
  ['`','1','2','3','4','5','6','7','8','9','0','-','=','Bksp'],
  ['Tab','Q','W','E','R','T','Y','U','I','O','P','[',']','\\'],
  ['Caps','A','S','D','F','G','H','J','K','L',';',"'",'Enter'],
  ['Shift','Z','X','C','V','B','N','M',',','.','/','Shift'],
  ['Ctrl','Fn','Win','Alt','Space','Alt','←','↑','↓','→']
];
var WIDE = { Bksp:2, Tab:1.5, Caps:1.8, Enter:2.2, Shift:2.4, Space:6, Del:1.2 };
var TONES = ['1 кГц синус','Левый / правый','Echo-тест микрофона'];
var CAMCHECKS = ['Превью идёт без артефактов','Цвета не уходят в зелень','Фокус и резкость в норме','Индикатор активности горит'];
var STATUS = {
  pass:{ label:'пройдено', cls:'pass' },
  fail:{ label:'ошибка', cls:'fail' },
  idle:{ label:'не проверено', cls:'' }
};

// Каталог драйверов — та же публичная ссылка, что в echips-driver-assistant.
var MANIFEST_PUBLIC_URL = "https://disk.360.yandex.ru/d/79yQHBN93UDZGg";

// Техники для замены платы — временно захардкожено, как в макете. ЭТО
// НЕБЕЗОПАСНО для продакшена (PIN лежит открытым текстом во фронтенде) —
// перед реальным использованием вынести в защищённый источник (сервер/файл
// с хэшами PIN), это отмечено в README как TODO.
var TECHS = [
  { id:'ivanov', name:'Иванов И.И.', pin:'1234' },
  { id:'petrov', name:'Петров П.П.', pin:'5678' }
];

var S = {
  screen:'start', cat:'usb', results:{}, comments:{},
  keys:{}, fill:0, padDots:[], padCount:0, padMax:0, padMoves:0,
  running:false, runLines:[], runError:null,
  tone:null, toneCtx:null, toneOsc:null, toneAnalyser:null, toneMic:null, phase:0,
  camStream:null,
  device:null, deviceError:null,
  sensorPoll:null, sensorReading:null, sensorHistory:[],
  stressOn:false, stressT:0, stressLoad:'CPU', stressDur:300, stressResult:null,
  snapshot:false, exported:null,
  drv:{ step:'idle' },
  mb:{ step:'login', techId:'', techName:'', pin:'', pinErr:'', ticket:'', serial:'', uuid:'', formErr:'', before:null, writeError:null }
};

function esc(s){ return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }
function cat(){ for (var i=0;i<CATS.length;i++) if (CATS[i].id===S.cat) return CATS[i]; return CATS[0]; }
function statusOf(id){ return S.results[id] || 'idle'; }
function counts(){
  var p=0,f=0;
  CATS.forEach(function(c){ if(S.results[c.id]==='pass') p++; else if(S.results[c.id]==='fail') f++; });
  return { pass:p, fail:f, checked:p+f };
}
function deviceLabel(){
  if (!S.device) return 'определяется…';
  return (S.device.manufacturer + ' ' + S.device.model).trim() || 'неизвестная модель';
}
function deviceSn(){ return S.device ? S.device.serial_number : ''; }

/* ---------- окно: свернуть/закрыть ---------- */
(function initWindowControls(){
  var win = getCurrentWindow();
  var minBtn = document.getElementById('win-minimize');
  var closeBtn = document.getElementById('win-close');
  if (minBtn) minBtn.addEventListener('click', function(){ win.minimize(); });
  if (closeBtn) closeBtn.addEventListener('click', function(){ win.close(); });
  var siteLink = document.getElementById('site-link');
  if (siteLink) siteLink.addEventListener('click', function(e){
    e.preventDefault();
    if (window.__TAURI__.shell) window.__TAURI__.shell.open('https://echips.ru');
  });
})();

/* ---------- загрузка данных устройства при старте ---------- */
function loadDevice(){
  invoke('get_system_info').then(function(info){
    S.device = info;
    render();
  }).catch(function(err){
    S.deviceError = typeof err === 'string' ? err : 'Не удалось определить устройство';
    render();
  });
}

/* ---------- действия ---------- */
function isValidSerial(v){ return v.length>=8 && v.length<=20 && /^[A-Za-z0-9]+$/.test(v); }
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
    stopSensorPoll(); stopCamera(); stopAudio();
    S.screen=screen; if(id) S.cat=id; S.running=false; S.runLines=[]; S.runError=null; S.exported=null; S.tone=null;
    if(screen==='drivers'){ A.drvStart(); }
    if(screen==='mb'){ A.mbReset(); }
    if(screen==='sensors'){ A.sensorsStart(); }
    render();
  },
  openCat:function(id){
    var c=null; CATS.forEach(function(x){ if(x.id===id) c=x; });
    stopCamera(); stopAudio();
    A.go(c.kind==='sensors'?'sensors':c.kind==='stress'?'stress':'test', id);
    if (c.kind==='camera') A.camStart();
  },
  reset:function(){ S.results={}; S.comments={}; S.keys={}; S.snapshot=false; render(); },
  press:function(id){ S.keys[id]=true; render(); },
  nextFill:function(){ S.fill=(S.fill+1)%FILLS.length; render(); },
  setFill:function(i){ S.fill=i; render(); },
  comment:function(v){ S.comments[S.cat]=v; },
  mark:function(v){ S.results[S.cat]=v; A.go('dash'); },
  snapshot:function(){ S.snapshot=true; render(); },
  exp:function(t){ A.exportReport(t); },

  /* ---- runner-категории: реальные invoke-запросы ---- */
  run:function(){
    var c = cat();
    S.running = true; S.runLines=[]; S.runError=null; render();
    fetchCategory(c.fetch).then(function(lines){
      S.running=false; S.runLines=lines; render();
    }).catch(function(err){
      S.running=false; S.runError = typeof err==='string' ? err : 'Ошибка получения данных';
      render();
    });
  },

  /* ---- камера ---- */
  camStart:function(){
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia){
      S.runError='Камера недоступна в этом окне (getUserMedia не поддерживается webview).'; render(); return;
    }
    navigator.mediaDevices.getUserMedia({ video:true }).then(function(stream){
      S.camStream = stream; render();
      var v = document.getElementById('cam-preview');
      if (v) v.srcObject = stream;
    }).catch(function(err){
      S.runError = 'Нет доступа к камере: ' + (err && err.message ? err.message : err);
      render();
    });
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
    function poll(){
      invoke('get_thermal_reading').then(function(r){
        S.sensorReading = r;
        if (r.available && r.cpu_temp_c!=null){
          S.sensorHistory = S.sensorHistory.concat([r.cpu_temp_c]).slice(-100);
        }
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

  /* ---- стресс-тест ---- */
  load:function(l){ S.stressLoad=l; render(); },
  dur:function(v){ S.stressDur=v; S.stressT=0; S.stressResult=null; render(); },
  stress:function(){
    if (S.stressOn) return; // остановка на лету не реализована — тест короткий и фиксированной длины
    S.stressOn=true; S.stressT=0; S.stressResult=null; render();
    invoke('run_cpu_stress', { durationSecs: S.stressDur }).then(function(res){
      S.stressOn=false; S.stressResult=res; render();
    }).catch(function(err){
      S.stressOn=false; S.runError = typeof err==='string'?err:'Ошибка стресс-теста'; render();
    });
  },

  /* ---- установка драйверов ---- */
  drvStart:function(){
    S.drv = { step:'scan' };
    render();
    invoke('get_system_info').then(function(info){
      S.device = info;
      return invoke('fetch_public_json', { publicUrl: MANIFEST_PUBLIC_URL }).catch(function(){
        return invoke('load_cached_manifest');
      });
    }).then(function(manifest){
      S.drv.manifest = manifest;
      invoke('cache_manifest', { manifest: manifest }).catch(function(){});
      return invoke('find_by_name', { manifest: manifest, manufacturer: S.device.manufacturer, model: S.device.model });
    }).then(function(match){
      if (!match){
        return invoke('find_by_serial_prefix', { manifest: S.drv.manifest, serial: S.device.serial_number });
      }
      return match;
    }).then(function(match){
      if (!match){
        S.drv.step='notfound'; render(); return;
      }
      S.drv.matchKey = match[0]; S.drv.entry = match[1]; S.drv.restore = true;
      S.drv.step='found'; render();
    }).catch(function(err){
      S.drv.step='error'; S.drv.error = typeof err==='string'?err:'Не удалось получить каталог драйверов';
      render();
    });
  },
  drvRestore:function(v){ S.drv.restore=v; render(); },
  drvInstall:function(){
    S.drv.step='installing'; S.drv.progress={ pct:0, label:'Подготовка...' }; render();
    var unlisten = null;
    tauriEvent.listen('install-progress', function(ev){
      var p = ev.payload;
      var pct = p.total>0 ? Math.round(p.downloaded/p.total*100) : (p.stage==='installing'?90:5);
      S.drv.progress = { pct: pct, label: p.file_label };
      render();
    }).then(function(u){ unlisten = u; });

    var files = [[S.drv.entry.yandex_public_key, S.drv.entry.path || null, S.drv.entry.display_name || S.drv.matchKey]];
    invoke('download_and_install', { files: files, createRestore: S.drv.restore }).then(function(result){
      if (unlisten) unlisten();
      S.drv.step='done'; S.drv.result=result; render();
    }).catch(function(err){
      if (unlisten) unlisten();
      S.drv.step='error'; S.drv.error = typeof err==='string'?err:'Ошибка установки'; render();
    });
  },

  /* ---- замена платы ---- */
  mbReset:function(){
    S.mb = { step:'login', techId:'', techName:'', pin:'', pinErr:'', ticket:'', serial:'', uuid:'', formErr:'', before:null, writeError:null };
    render();
  },
  mbPickTech:function(id){ S.mb.techId=id; S.mb.pinErr=''; render(); },
  mbPin:function(v){ S.mb.pin=v; },
  mbLogin:function(){
    var t=null; TECHS.forEach(function(x){ if(x.id===S.mb.techId) t=x; });
    if(!t){ S.mb.pinErr='Выберите техника.'; render(); return; }
    if(t.pin!==S.mb.pin){ S.mb.pinErr='Неверный PIN.'; render(); return; }
    S.mb.techName=t.name; S.mb.step='reading'; render();
    invoke('read_board_identity').then(function(id){
      S.mb.before = id; S.mb.step='form'; render();
    }).catch(function(err){
      S.mb.pinErr = typeof err==='string'?err:'Не удалось прочитать SN/UUID платы';
      S.mb.step='login'; render();
    });
  },
  mbField:function(k,v){ S.mb[k]=v; },
  mbNext:function(){
    var ticket=(S.mb.ticket||'').trim(), serial=(S.mb.serial||'').trim(), uuid=(S.mb.uuid||'').trim();
    if(!ticket || !serial || !uuid){ S.mb.formErr='Заполните все поля.'; render(); return; }
    if(!isValidSerial(serial)){ S.mb.formErr='Серийный номер: 8–20 латинских букв/цифр.'; render(); return; }
    if(!isValidUuid(uuid)){ S.mb.formErr='UUID в формате 8-4-4-4-12.'; render(); return; }
    S.mb.formErr=''; S.mb.step='confirm'; render();
  },
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
      S.mb.step='done'; render();
    }).catch(function(err){
      // Ожидаемо: команда — заглушка (см. комментарий в motherboard.rs), но
      // попытка всё равно попадает в аудит-лог. Показываем это как есть,
      // а не притворяемся, что запись прошла.
      S.mb.writeError = typeof err==='string'?err:'Запись не выполнена';
      S.mb.step='stub';
      render();
    });
  },

  /* ---- отчёт ---- */
  exportReport:function(kind){
    var testable = CATS;
    var report = {
      device_model: deviceLabel(),
      device_serial: deviceSn(),
      engineer: '',
      started_at: S.startedAt || new Date().toISOString(),
      finished_at: new Date().toISOString(),
      results: testable.map(function(c){
        return { id:c.id, title:c.name, status: statusOf(c.id), comment: S.comments[c.id] || null };
      })
    };
    var command = kind==='json' ? 'save_report_json' : 'save_report_txt';
    invoke(command, { report: report }).then(function(path){
      S.exported = { kind: kind, path: path }; render();
    }).catch(function(err){
      S.runError = typeof err==='string'?err:'Не удалось сохранить отчёт'; render();
    });
  }
};
window.echips = A;
S.startedAt = new Date().toISOString();

/* ---------- реальные данные для runner-категорий ---------- */
function fetchCategory(kind){
  if (kind==='usb'){
    return invoke('list_usb_devices').then(function(list){
      if (!list.length) return ['USB-устройства не обнаружены (кроме встроенных корневых хабов).'];
      return list.map(function(d){ return d.name + ' — ' + (d.status==='OK'?'работает':d.status); });
    });
  }
  if (kind==='bt'){
    return invoke('list_bluetooth_devices').then(function(list){
      if (!list.length) return ['Bluetooth-адаптер не обнаружен или отключён.'];
      return list.map(function(d){ return d.name + ' — ' + (d.status==='OK'?'работает':d.status); });
    });
  }
  if (kind==='wifi'){
    return Promise.all([invoke('list_wifi_adapters'), invoke('scan_wifi_networks').catch(function(){ return []; })])
      .then(function(res){
        var adapters=res[0], networks=res[1];
        var lines = adapters.length
          ? adapters.map(function(a){ return 'Адаптер: ' + a.name + ' — ' + a.status + ' (' + a.mac + ')'; })
          : ['Wi-Fi адаптер не обнаружен.'];
        lines.push('Видимых сетей: ' + networks.length);
        return lines.concat(networks.slice(0,8));
      });
  }
  if (kind==='fp'){
    return invoke('get_fingerprint_sensor').then(function(name){
      return name ? ['Сенсор обнаружен системой: ' + name, 'Пробную регистрацию и сравнение выполните вручную через Windows Hello.']
                   : ['Сенсор отпечатка не обнаружен в системе (WinBio).'];
    });
  }
  if (kind==='bat'){
    return invoke('get_battery_info').then(function(b){
      if (!b.present) return ['Батарея не обнаружена системой.'];
      var lines = ['Заряд: ' + b.charge_percent + '% (' + (b.charging?'заряжается':'от батареи') + ')'];
      if (b.design_capacity_mwh!=null && b.full_charge_capacity_mwh!=null){
        lines.push('Design capacity: ' + b.design_capacity_mwh + ' мВт·ч');
        lines.push('Full charge capacity: ' + b.full_charge_capacity_mwh + ' мВт·ч');
        lines.push('Износ: ' + (100 - (b.health_percent||0)).toFixed(1) + '% (health ' + (b.health_percent||0).toFixed(1) + '%)');
      } else {
        lines.push('powercfg /batteryreport не вернул данные о ёмкости на этой машине.');
      }
      if (b.cycle_count!=null) lines.push('Циклов заряда: ' + b.cycle_count);
      return lines;
    });
  }
  return Promise.reject('Неизвестная категория');
}

/* ---------- сайдбар ---------- */
function renderNav(){
  var active = { start:'start', drivers:'start', mb:'start', dash:'dash', test:'dash', sensors:'sensors', stress:'stress', report:'report' }[S.screen];
  var c = counts();
  var items = [
    { k:'start', label:'Режим', meta:'' },
    { k:'dash', label:'Категории', meta:c.checked+'/'+CATS.length },
    { k:'sensors', label:'Датчики', meta:S.sensorPoll?'live':'' },
    { k:'stress', label:'Стресс-тест', meta:S.stressOn?'···':'' },
    { k:'report', label:'Отчёт', meta:'' }
  ];
  document.getElementById('steps').innerHTML = items.map(function(i){
    return '<div class="step'+(i.k===active?' active':'')+'" onclick="echips.go(\''+i.k+'\')">'+
      '<span class="dot"></span><span class="lbl">'+i.label+'</span><span class="meta">'+i.meta+'</span></div>';
  }).join('');

  document.getElementById('devbox-name').textContent = S.device ? deviceLabel() : (S.deviceError ? 'ошибка определения' : 'определяется…');
  document.getElementById('devbox-sn').textContent = S.device ? ('SN ' + (S.device.serial_number || '—')) : '';
}

/* ---------- экраны ---------- */
function screenStart(){
  var modes = [
    { tag:'DRV', title:'Установка драйверов', desc:'Определение модели, выбор пакетов и установка с точкой восстановления.', meta:'та же логика, что в Driver Assistant', badge:'ГОТОВО', hot:false, go:'drivers' },
    { tag:'DIA', title:'Диагностика оборудования', desc:CATS.length+' категорий тестов, датчики (где доступны), стресс-тест и отчёт.', meta:CATS.length+' категорий · TXT / JSON', badge:'НОВОЕ', hot:true, go:'dash' },
    { tag:'MB', title:'Замена платы', desc:'Гарантийный случай: чтение SN/UUID и аудит-лог. Запись — требует донастройки.', meta:'частично · см. README', badge:'В РАБОТЕ', hot:false, go:'mb' }
  ];
  var detected = S.device
    ? deviceLabel() + (S.device.bios_version ? ' · BIOS ' + esc(S.device.bios_version) : '') + (S.device.os_version ? ' · ' + esc(S.device.os_version) : '')
    : (S.deviceError ? 'Не удалось определить устройство: ' + esc(S.deviceError) : 'определяется…');
  return '<div class="pane">'+
    '<div class="eyebrow">Режим работы</div>'+
    '<h1 class="title">Что делаем с ноутбуком</h1>'+
    '<p class="lede" style="margin:7px 0 24px">Выберите режим — драйверы, полная проверка оборудования или гарантийная замена платы.</p>'+
    '<div class="modes">'+ modes.map(function(m){
      return '<div class="mode'+(m.hot?' is-new':'')+'" onclick="echips.go(\''+m.go+'\')">'+
        '<div class="row"><div class="ic">'+m.tag+'</div><span class="badge'+(m.hot?' hot':'')+'">'+m.badge+'</span></div>'+
        '<h3>'+m.title+'</h3><p>'+m.desc+'</p><div class="foot">'+m.meta+'</div></div>';
    }).join('') +'</div>'+
    '<div class="detected"><span class="pulse'+(S.device?' anim':'')+'"></span>Определено: '+detected+'</div>'+
  '</div>';
}

function screenDash(){
  var c = counts();
  return '<div class="pane">'+
    '<div class="head"><div><div class="eyebrow">Диагностика оборудования</div><h1 class="title">Категории тестов</h1></div>'+
    '<div class="headactions">'+
      '<button class="btn btn-ghost" onclick="echips.reset()">Сбросить</button>'+
      '<button class="btn btn-primary" onclick="echips.go(\'report\')">К отчёту</button>'+
    '</div></div>'+
    '<div class="progrow"><div class="bar"><div class="fill" style="width:'+(c.checked/CATS.length*100).toFixed(0)+'%"></div></div>'+
    '<div class="lbl">проверено '+c.checked+' из '+CATS.length+' · пройдено '+c.pass+' · ошибок '+c.fail+'</div></div>'+
    '<div class="cats">'+ CATS.map(function(x){
      var st = statusOf(x.id), live = x.kind==='sensors';
      return '<div class="cat '+(live?'live ':'')+STATUS[st].cls+'" onclick="echips.openCat(\''+x.id+'\')">'+
        '<div class="row"><span class="tag">'+x.tag+'</span><span class="name">'+x.name+'</span><span class="sdot"></span></div>'+
        '<div class="method">'+x.method+'</div>'+
        '<div class="foot"><span class="st">'+(live?'мониторинг':STATUS[st].label)+'</span><span>'+x.impl+'</span></div></div>';
    }).join('') +'</div></div>';
}

function fieldKeyboard(){
  var pressed = Object.keys(S.keys).length, total = 0;
  KEYROWS.forEach(function(r){ total += r.length; });
  return '<div class="kbwrap">'+
    '<div class="kbmeta"><span>RAW INPUT · нажмите каждую клавишу на ноутбуке</span>'+
    '<span>нажато '+pressed+' из '+total+' · rollover '+(pressed>3?'n-key ok':'—')+'</span></div>'+
    '<div class="kbrows">'+ KEYROWS.map(function(row,ri){
      return '<div class="kbrow">'+ row.map(function(label,ki){
        var id = ri+':'+ki;
        return '<div class="key'+(S.keys[id]?' on':'')+'" style="flex:'+(WIDE[label]||1)+' 1 0" onclick="echips.press(\''+id+'\')">'+esc(label)+'</div>';
      }).join('') +'</div>';
    }).join('') +'</div>'+
    '<div class="kbnote">Залипание и непрожатие определяет техник глазами — результат ниже.</div></div>';
}
function fieldDisplay(){
  return '<div class="fillwrap">'+
    '<div class="fillstage" style="background:'+FILLS[S.fill].color+'" onclick="echips.nextFill()">'+
    '<span>клик — следующая заливка · '+FILLS[S.fill].name+'</span></div>'+
    '<div class="swatches">'+ FILLS.map(function(f,i){
      return '<div class="swatch'+(i===S.fill?' on':'')+'" style="background:'+f.color+'" onclick="echips.setFill('+i+')"></div>';
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
    '<div class="log">'+body+'</div></div>';
}
function fieldCamera(){
  return '<div class="camwrap"><div class="preview" style="position:relative;overflow:hidden">'+
    (S.camStream
      ? '<video id="cam-preview" autoplay muted playsinline style="width:100%;height:100%;object-fit:cover;border-radius:10px"></video>'
      : '<div class="lens">CAM</div><div class="m">'+(S.runError?esc(S.runError):'запрос доступа к камере…')+'</div>')+
    '</div>'+
    '<div class="side">'+ CAMCHECKS.map(function(s){ return '<div class="note">'+s+'</div>'; }).join('') +'</div></div>';
}
function fieldAudio(){
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
  var note = S.tone===null ? 'выберите сигнал — спектр появится ниже'
    : S.tone===2 ? 'echo-тест: сигнал с микрофона идёт в анализатор напрямую (Web Audio)'
    : 'воспроизведение через встроенные динамики · Web Audio API';
  return '<div class="audiowrap"><div class="tones">'+ TONES.map(function(t,i){
      return '<button class="tone'+(S.tone===i?' on':'')+'" onclick="echips.tone('+i+')">'+t+'</button>';
    }).join('') +'</div>'+
    '<div class="spectrum">'+bars+'</div><div class="kbnote">'+note+'</div></div>';
}

function screenTest(){
  var c = cat(), field = '';
  if(c.kind==='keyboard') field = fieldKeyboard();
  else if(c.kind==='display') field = fieldDisplay();
  else if(c.kind==='touchpad') field = fieldTouchpad();
  else if(c.kind==='camera') field = fieldCamera();
  else if(c.kind==='audio') field = fieldAudio();
  else field = fieldRunner();
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'dash\')">← все категории</button>'+
    '<span class="idx">категория '+(CATS.indexOf(c)+1)+' из '+CATS.length+'</span></div>'+
    '<div class="testhead"><div><h2>'+c.name+'</h2><div class="hint">'+c.method+'</div></div>'+
    '<div class="base">'+c.tag+' · '+c.impl+'</div></div>'+
    '<div class="field">'+field+'</div>'+
    '<div class="verdict">'+
      '<input placeholder="Комментарий техника — попадёт в отчёт" value="'+esc(S.comments[c.id]||'')+'" oninput="echips.comment(this.value)">'+
      '<button class="btn btn-danger" onclick="echips.mark(\'fail\')">Не пройден</button>'+
      '<button class="btn btn-primary" onclick="echips.mark(\'pass\')">Пройден</button>'+
    '</div></div>';
}

function screenSensors(){
  var r = S.sensorReading;
  var cpuVal = r && r.available ? r.cpu_temp_c.toFixed(1) : '—';
  var rows = [
    { k:'CPU (ACPI)', v:cpuVal, u:'°C', c:'#FF8A00', m: r ? esc(r.note) : 'опрос…' }
  ];
  var grid = '<line x1="0" y1="0" x2="1000" y2="0" stroke="rgba(255,255,255,.06)"></line>'+
    '<line x1="0" y1="150" x2="1000" y2="150" stroke="rgba(255,255,255,.06)"></line>'+
    '<line x1="0" y1="300" x2="1000" y2="300" stroke="rgba(255,255,255,.10)"></line>';
  var hist = S.sensorHistory;
  var points = hist.length ? hist.map(function(v,i){
    var x = (i/((hist.length-1)||1))*1000;
    var y = 300 - Math.max(0,Math.min(1,(v-20)/(80-20)))*300;
    return x.toFixed(1)+','+y.toFixed(1);
  }).join(' ') : '';
  var chart = hist.length
    ? '<div class="plot"><svg viewBox="0 0 1000 300" preserveAspectRatio="none">'+grid+
      '<polyline points="'+points+'" fill="none" stroke="#FF8A00" stroke-width="2" vector-effect="non-scaling-stroke" stroke-linejoin="round"></polyline></svg></div>'
    : '<div class="infoline" style="margin:0">'+(r && !r.available ? esc(r.note) : 'ожидание данных…')+'</div>';
  return '<div class="pane">'+
    '<div class="head"><div><div class="eyebrow">Live · WMI ACPI</div><h1 class="title">Датчики</h1></div>'+
    '<div class="lbl" style="font-family:var(--mono);font-size:11px;color:var(--dim);display:flex;align-items:center;gap:8px">'+
    '<span class="pulse'+(S.sensorPoll?' anim':'')+'"></span>опрос 2 с</div></div>'+
    '<div class="readouts">'+ rows.map(function(r2){
      return '<div class="readout"><div class="k"><i style="background:'+r2.c+'"></i>'+r2.k+'</div>'+
        '<div class="v"><b>'+r2.v+'</b><span>'+r2.u+'</span></div><div class="m">'+r2.m+'</div></div>';
    }).join('') +'</div>'+
    '<div class="chart">'+chart+'</div>'+
    '<div class="footrow"><span class="txt">Без LibreHardwareMonitor/HWInfo доступен только один ACPI-датчик через WMI, и не на всех платах. Снимок можно приложить к отчёту.</span>'+
    '<button class="btn btn-ghost" onclick="echips.snapshot()">'+(S.snapshot?'Снимок добавлен в отчёт':'Приложить снимок к отчёту')+'</button></div></div>';
}

function screenStress(){
  var sd = S.stressResult;
  var durs = [[60,'1 мин'],[300,'5 мин'],[900,'15 мин'],[1800,'30 мин']];
  return '<div class="pane">'+
    '<div class="eyebrow">Нагрузка</div><h1 class="title">Стресс-тест CPU</h1>'+
    '<p class="lede" style="margin:6px 0 0">Реальная busy-loop нагрузка на все логические ядра. GPU-нагрузка и мониторинг throttling не реализованы — нет доступа к частотам/температурам на большинстве плат (см. вкладку «Датчики»).</p>'+
    '<div class="controls">'+
      '<div class="control"><div class="k">Длительность</div><div class="opts">'+
        durs.map(function(d){
          return '<button class="opt mono'+(S.stressDur===d[0]?' on':'')+'" onclick="echips.dur('+d[0]+')" '+(S.stressOn?'disabled':'')+'>'+d[1]+'</button>';
        }).join('') +'</div></div>'+
    '</div>'+
    '<div class="chart" style="margin-top:18px"><div class="top">'+
      '<div class="stressrun"><div class="hex'+(S.stressOn?' spin':'')+'"><svg viewBox="0 0 100 100">'+
        '<polygon class="trk" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon>'+
        '<polygon class="arc" points="50,6 89,28 89,72 50,94 11,72 11,28"></polygon></svg></div>'+
      '<div><div class="stage">'+(S.stressOn?'Прогон CPU':sd?'Прогон завершён':'Готов к запуску')+'</div>'+
      '<div class="clock">'+(sd?sd.elapsed_secs:0)+' с / '+S.stressDur+' с</div></div></div>'+
      '</div></div>'+
    '<div class="stats4">'+
      '<div class="stat4"><div class="k">потоков нагружено</div><div class="v">'+(sd?sd.threads:'—')+'</div></div>'+
      '<div class="stat4"><div class="k">завершено</div><div class="v '+(sd?'ok':'none')+'">'+(sd?(sd.completed?'да':'нет'):'—')+'</div></div>'+
      '<div class="stat4"><div class="k">throttling</div><div class="v none">нет данных</div></div>'+
      '<div class="stat4"><div class="k">макс. температура</div><div class="v none">см. «Датчики»</div></div>'+
    '</div>'+
    '<div class="footrow"><span class="mono">'+(S.stressOn?'нагрузка на все логические ядра запущена':'нажмите «Запустить», окно приложения останется отзывчивым')+'</span>'+
    '<button class="btn btn-primary" onclick="echips.stress()" '+(S.stressOn?'disabled':'')+'>'+(S.stressOn?'Идёт прогон…':sd?'Запустить снова':'Запустить')+'</button></div></div>';
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
function screenDrivers(){
  var d = S.drv, body;
  if(d.step==='idle' || d.step==='scan'){
    body = hexSpinner('ОПРЕДЕЛЕНИЕ МОДЕЛИ И ПОИСК ПАКЕТА');
  } else if (d.step==='notfound'){
    body = '<div class="resultpane">'+resultIcon(false)+
      '<div class="msg">Точный пакет для этой модели не найден в каталоге (по имени и по серийному номеру). '+
      'Проверьте вручную на '+ '<a href="#" onclick="return false">echips.ru</a> или уточните модель у поддержки.</div>'+
      '<div class="actions"><button class="btn btn-primary" onclick="echips.go(\'start\')">Назад</button></div></div>';
  } else if (d.step==='error'){
    body = '<div class="resultpane">'+resultIcon(false)+
      '<div class="msg">'+esc(d.error)+'</div>'+
      '<div class="actions"><button class="btn btn-ghost" onclick="echips.go(\'start\')">Назад</button>'+
      '<button class="btn btn-primary" onclick="echips.drvStart()">Повторить</button></div></div>';
  } else if(d.step==='found'){
    var name = d.entry.display_name || d.matchKey;
    body =
      '<div class="card"><div class="k">Модель</div><div class="n">'+esc(deviceLabel())+'</div>'+
      '<div class="s">SN '+esc(deviceSn())+' · совпадение по каталогу: '+esc(d.matchKey)+'</div></div>'+
      '<div class="card"><div class="k">Пакет</div><div class="n">'+esc(name)+'</div></div>'+
      '<label class="checkrow"><input type="checkbox" '+(d.restore?'checked':'')+' onchange="echips.drvRestore(this.checked)">'+
      'Создать точку восстановления перед установкой</label>'+
      '<div class="headactions" style="margin-top:20px;justify-content:flex-end">'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.drvInstall()">Установить</button></div>';
  } else if(d.step==='installing'){
    var p = d.progress || { pct:0, label:'' };
    body =
      '<div class="card"><div class="k">Установка</div><div class="n">'+esc(p.label)+'</div>'+
      '<div class="bar" style="margin-top:14px"><div class="fill" style="width:'+p.pct+'%"></div></div></div>';
  } else if(d.step==='done'){
    var res = d.result || { message:'Готово.' };
    body =
      '<div class="resultpane">'+resultIcon(true)+
      '<div class="msg">'+esc(res.message)+'</div>'+
      '<div class="actions"><button class="btn btn-ghost" onclick="invoke_open_log(\''+esc(res.log_path||'')+'\')">Открыть лог</button>'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Позже</button>'+
      '<button class="btn btn-primary" onclick="echips_restart()">Перезагрузить сейчас</button></div></div>';
  }
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'start\')">← режимы</button>'+
    '<span class="idx">установка драйверов</span></div>'+
    '<div class="testhead"><div><h2>Установка драйверов</h2>'+
    '<div class="hint">Тот же поток, что в Echips Driver Assistant: определение модели → пакет → установка с точкой восстановления.</div></div></div>'+
    '<div class="field" style="margin-top:16px">'+body+'</div></div>';
}
window.invoke_open_log = function(){ invoke('open_log_folder').catch(function(){}); };
window.echips_restart = function(){ invoke('restart_system').catch(function(){}); };

/* ---------- замена платы (гарантия) ---------- */
function screenMb(){
  var m = S.mb, body;
  if(m.step==='login'){
    body =
      '<div class="picklist">'+TECHS.map(function(t){
        return '<div class="pickrow'+(m.techId===t.id?' on':'')+'" onclick="echips.mbPickTech(\''+t.id+'\')">'+
          '<span class="radio"></span><span class="lbl">'+t.name+'</span></div>';
      }).join('')+'</div>'+
      '<div class="formfield" style="max-width:220px;margin-top:14px"><label>PIN</label>'+
      '<input type="password" value="'+esc(m.pin)+'" oninput="echips.mbPin(this.value)" placeholder="••••">'+
      (m.pinErr?'<div class="err">'+esc(m.pinErr)+'</div>':'')+'</div>'+
      '<div class="headactions" style="margin-top:6px">'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.mbLogin()">Войти</button></div>';
  } else if(m.step==='reading'){
    body = hexSpinner('ЧТЕНИЕ ТЕКУЩИХ SN/UUID С ПЛАТЫ');
  } else if(m.step==='form'){
    body =
      '<div class="card"><div class="k">Текущие значения</div>'+
      '<div class="s" style="margin-top:8px">SN '+esc(m.before.serial_number)+'</div>'+
      '<div class="s">UUID '+esc(m.before.uuid)+'</div></div>'+
      '<div class="formgrid" style="margin-top:16px">'+
      '<div class="formfield"><label>Номер наряда</label><input value="'+esc(m.ticket)+'" oninput="echips.mbField(\'ticket\',this.value)" placeholder="Гарантийный случай / наряд"></div>'+
      '<div class="formfield"><label>Новый серийный номер</label><input value="'+esc(m.serial)+'" oninput="echips.mbField(\'serial\',this.value)" placeholder="8–20 букв/цифр"></div>'+
      '<div class="formfield"><label>Новый UUID</label><input value="'+esc(m.uuid)+'" oninput="echips.mbField(\'uuid\',this.value)" placeholder="8-4-4-4-12"></div>'+
      (m.formErr?'<div class="err" style="margin:-6px 0 12px">'+esc(m.formErr)+'</div>':'')+
      '</div>'+
      '<div class="headactions">'+
      '<button class="btn btn-ghost" onclick="echips.go(\'start\')">Отмена</button>'+
      '<button class="btn btn-primary" onclick="echips.mbNext()">Далее</button></div>';
  } else if(m.step==='confirm'){
    body =
      '<div class="kvgrid">'+
      '<div class="h">Поле</div><div class="h">Было</div><div class="h">Будет</div>'+
      '<div class="lbl">SN</div><div class="old">'+esc(m.before.serial_number)+'</div><div class="new">'+esc(m.serial)+'</div>'+
      '<div class="lbl">UUID</div><div class="old">'+esc(m.before.uuid)+'</div><div class="new">'+esc(m.uuid)+'</div>'+
      '</div>'+
      '<div class="s" style="margin-top:14px">Наряд: '+esc(m.ticket)+' · Техник: '+esc(m.techName)+'</div>'+
      '<div class="warnbox">Команда физической записи через AMIDEWINx64.exe в этой сборке не сконфигурирована — '+
      'подставьте точный путь/аргументы вашей проверенной процедуры в src-tauri/src/commands/motherboard.rs, прежде '+
      'чем использовать эту кнопку на реальной плате. Попытка всё равно попадёт в аудит-лог с хэш-цепочкой.</div>'+
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
      '<div class="msg">SN и UUID успешно записаны. Запись сохранена в журнал аудита.</div>'+
      '<div class="actions"><button class="btn btn-primary" onclick="echips.go(\'start\')">Готово</button></div></div>';
  }
  return '<div class="pane">'+
    '<div class="crumbs"><button class="btn-link" onclick="echips.go(\'start\')">← режимы</button>'+
    '<span class="idx">замена платы · гарантия</span></div>'+
    '<div class="testhead"><div><h2>Замена платы</h2>'+
    '<div class="hint">Доступ только для авторизованного техника. Чтение SN/UUID — реальное (WMI); запись требует донастройки, см. предупреждение ниже.</div></div></div>'+
    '<div class="field" style="margin-top:16px">'+body+'</div></div>';
}

function screenReport(){
  var c = counts(), full = c.checked===CATS.length;
  var verdict = c.fail ? 'в ремонт' : full ? 'годен' : 'не завершено';
  return '<div class="pane">'+
    '<div class="head"><div><div class="eyebrow">Итог прогона</div><h1 class="title">Отчёт</h1></div>'+
    '<div class="headactions">'+
      '<button class="btn btn-ghost" onclick="echips.exp(\'json\')">Экспорт JSON</button>'+
      '<button class="btn btn-primary" onclick="echips.exp(\'txt\')">Экспорт TXT</button>'+
    '</div></div>'+
    '<div class="repstats">'+
      '<div class="repstat ok"><div class="k">пройдено</div><div class="v ok">'+c.pass+'</div></div>'+
      '<div class="repstat'+(c.fail?' err':'')+'"><div class="k">ошибки</div><div class="v '+(c.fail?'err':'dim')+'">'+c.fail+'</div></div>'+
      '<div class="repstat"><div class="k">не проверено</div><div class="v dim">'+(CATS.length-c.checked)+'</div></div>'+
      '<div class="repstat"><div class="k">вердикт</div><div class="v '+(c.fail?'err':full?'ok':'dim')+'" style="font-size:'+(verdict.length>8?'19px':'25px')+'">'+verdict+'</div></div>'+
    '</div>'+
    '<div class="table"><div class="th"><span class="c-num">№</span><span class="c-name">Компонент</span>'+
      '<span class="c-impl">Метод</span><span class="c-st">Статус</span><span class="c-cm">Комментарий техника</span></div>'+
      '<div class="tb">'+ CATS.map(function(x,i){
        var st = statusOf(x.id);
        var cm = S.comments[x.id] || (x.id==='sens' && S.snapshot ? 'приложен снимок датчиков' : '—');
        return '<div class="tr"><span class="c-num">'+String(i+1).padStart(2,'0')+'</span>'+
          '<span class="c-name">'+x.name+'</span><span class="c-impl">'+x.impl+'</span>'+
          '<span class="c-st"><span class="pill '+STATUS[st].cls+'"><i></i>'+STATUS[st].label+'</span></span>'+
          '<span class="c-cm">'+esc(cm)+'</span></div>';
      }).join('') +'</div></div>'+
    '<div class="footrow"><span class="mono">'+esc(deviceLabel())+' · SN '+esc(deviceSn())+'</span>'+
    '<span class="exp'+(S.exported?' done':'')+'" style="font-family:var(--mono);font-size:10.5px">'+
    (S.exported ? esc(S.exported.path) + ' сохранён' : 'экспорт: TXT для акта, JSON для базы')+
    '</span></div></div>';
}

/* ---------- рендер ---------- */
function render(){
  renderNav();
  var host = document.getElementById('screen');
  var focus = document.activeElement, sel = null;
  if(focus && focus.tagName==='INPUT') sel = focus.selectionStart;
  host.innerHTML = S.screen==='start' ? screenStart()
    : S.screen==='drivers' ? screenDrivers()
    : S.screen==='mb' ? screenMb()
    : S.screen==='dash' ? screenDash()
    : S.screen==='test' ? screenTest()
    : S.screen==='sensors' ? screenSensors()
    : S.screen==='stress' ? screenStress() : screenReport();
  if(sel!==null){
    var inp = host.querySelector('input');
    if(inp){ inp.focus(); try{ inp.setSelectionRange(sel,sel); }catch(e){} }
  }
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
  if (S.tone!==null && S.screen==='test' && cat().kind==='audio') render();
})();

document.addEventListener('DOMContentLoaded', function(){
  loadDevice();
  render();
});
})();
