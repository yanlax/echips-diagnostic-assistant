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
  { id:'sys', tag:'SYS', name:'Системная информация', method:'Процессор, ОЗУ, диски, видеокарта, плата, BIOS + сверка с профилем модели', impl:'реальные данные', kind:'runner', fetch:'sys' },
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
  na:{ label:'не применимо', cls:'na' },
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
  hw:null, verdict:null,
  auto:{ on:false, ids:[], idx:-1, stopped:false, waiting:false, msg:'', cls:'' },
  drv:{ step:'idle' },
  mb:{ step:'login', techId:'', techName:'', pin:'', pinErr:'', ticket:'', serial:'', uuid:'', formErr:'', before:null, writeError:null }
};

function esc(s){ return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }
function cat(){ for (var i=0;i<CATS.length;i++) if (CATS[i].id===S.cat) return CATS[i]; return CATS[0]; }
function statusOf(id){ return S.results[id] || 'idle'; }
function counts(){
  var p=0,f=0,n=0;
  CATS.forEach(function(c){
    if(S.results[c.id]==='pass') p++; else if(S.results[c.id]==='fail') f++; else if(S.results[c.id]==='na') n++;
  });
  return { pass:p, fail:f, na:n, checked:p+f+n };
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
    invoke('get_hardware_summary').then(function(hw){ S.hw = hw; render(); }).catch(function(){});
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
    if (document.getElementById('fill-overlay')) A.fillClose();
    if (S.auto.on && screen!=='test' && screen!=='report') A.autoOff();
    S.screen=screen; if(id) S.cat=id; S.running=false; S.runLines=[]; S.runError=null; S.verdict=null; S.exported=null; S.tone=null;
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
    if (c.kind==='runner' && S.auto.on) A.run();
  },
  reset:function(){ A.autoOff(); S.results={}; S.comments={}; S.keys={}; S.snapshot=false; render(); },
  press:function(id){ S.keys[id]=true; render(); },
  nextFill:function(){ S.fill=(S.fill+1)%FILLS.length; render(); paintFill(); },
  prevFill:function(){ S.fill=(S.fill+FILLS.length-1)%FILLS.length; render(); paintFill(); },
  fillOpen:function(){
    if (document.getElementById('fill-overlay')) return;
    var o = document.createElement('div');
    o.id = 'fill-overlay';
    o.innerHTML = '<span id="fill-hint"></span>';
    o.addEventListener('click', function(){ A.nextFill(); });
    document.body.appendChild(o);
    try { getCurrentWindow().setFullscreen(true); } catch(e){}
    paintFill();
    clearTimeout(S.fillHintT);
    S.fillHintT = setTimeout(function(){ var h=document.getElementById('fill-hint'); if(h) h.style.opacity='0'; }, 3500);
  },
  fillClose:function(){
    var o = document.getElementById('fill-overlay');
    if (o) o.parentNode.removeChild(o);
    try { getCurrentWindow().setFullscreen(false); } catch(e){}
    render();
  },
  setFill:function(i){ S.fill=i; render(); },
  comment:function(v){ S.comments[S.cat]=v; },
  mark:function(v){
    S.results[S.cat]=v;
    if (S.auto.on) A.autoAfter(v); else A.go('dash');
  },
  snapshot:function(){ S.snapshot=true; render(); },
  exp:function(t){ A.exportReport(t); },

  /* ---- автопрогон по профилю модели ---- */
  autoStart:function(){
    var ids = (profile().tests||[]).filter(function(id){ return CATS.some(function(c){ return c.id===id; }); });
    if (!ids.length) return;
    S.results={}; S.comments={}; S.keys={}; S.snapshot=false;
    S.auto = { on:true, ids:ids, idx:-1, stopped:false, waiting:false, msg:'', cls:'', timer:null };
    A.autoNext();
  },
  autoOff:function(){
    if (S.auto.timer) clearTimeout(S.auto.timer);
    S.auto = { on:false, ids:[], idx:-1, stopped:false, waiting:false, msg:'', cls:'' };
  },
  autoStop:function(){ A.autoOff(); A.go('dash'); },
  autoReport:function(){ A.autoOff(); A.go('report'); },
  autoNext:function(){
    var a = S.auto; if (!a.on) return;
    if (a.timer) clearTimeout(a.timer);
    a.idx++; a.stopped=false; a.waiting=false; a.msg=''; a.cls='';
    if (a.idx >= a.ids.length){ A.autoReport(); return; }
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
    var at = a.idx;
    if (v.status==='fail'){
      if (profile().stopAtFail){ a.stopped=true; a.msg='Не пройден: '+v.note+' — автопрогон остановлен.'; a.cls='err'; }
      else { a.waiting=true; a.msg='Не пройден: '+v.note; a.cls='err'; }
      render(); return;
    }
    a.msg=(v.status==='na'?'Не применимо: ':'Пройден: ')+v.note+' · переход к следующему…'; a.cls='ok'; render();
    a.timer = setTimeout(function(){ if (S.auto.on && S.auto.idx===at) A.autoNext(); }, 1800);
  },

  /* ---- runner-категории: реальные invoke-запросы ---- */
  run:function(){
    var c = cat();
    S.running = true; S.runLines=[]; S.runError=null; render();
    S.verdict = null;
    fetchCategory(c.fetch).then(function(res){
      S.running=false; S.runLines=res.lines; S.verdict=res.verdict; render();
      if (S.auto.on && S.cat===c.id) A.autoApply(res.verdict);
    }).catch(function(err){
      S.running=false; S.runError = typeof err==='string' ? err : 'Ошибка получения данных';
      render();
      if (S.auto.on && S.cat===c.id) A.autoApply({ status:'fail', note:S.runError });
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
    S.drv = { step:'scan', scanLabel:'СКАНИРОВАНИЕ ОБОРУДОВАНИЯ', restore:true };
    render();
    invoke('get_system_info').then(function(info){
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
    ? { status:'fail', note: what+' не обнаружен — обязателен для этой модели' }
    : { status:'na', note: what+' не обнаружен в системе' };
}

function sysReport(hw){
  var e = profile().expect || {}, lines = [], bad = [];
  function chk(label, val, ok, exp){
    lines.push((ok===null ? '•' : ok ? '✓' : '✗') + ' ' + label + ': ' + val + (exp!=null && exp!=='' ? ' (ожидается ' + exp + ')' : ''));
    if (ok===false) bad.push(label);
  }
  function near(v, exp){ return exp>0 && Math.abs(v-exp)/exp <= 0.10; }
  chk('Процессор', hw.cpu.name+' · '+hw.cpu.cores+' ядер / '+hw.cpu.threads+' потоков'+(hw.cpu.max_mhz?' · '+hw.cpu.max_mhz+' МГц':''),
    e.cpu ? hw.cpu.name.toLowerCase().indexOf(String(e.cpu).toLowerCase())>=0 : null, e.cpu);
  chk('ОЗУ', hw.ram_total_gb.toFixed(1)+' ГБ, модулей: '+hw.ram_modules.length, e.ramGb ? near(hw.ram_total_gb, e.ramGb) : null, e.ramGb ? e.ramGb+' ГБ' : '');
  hw.ram_modules.forEach(function(m){
    lines.push('    '+m.slot+': '+m.capacity_gb+' ГБ'+(m.speed_mhz?' · '+m.speed_mhz+' МГц':'')+(m.manufacturer?' · '+m.manufacturer:'')+(m.part_number?' · '+m.part_number:''));
  });
  var biggest = hw.disks.reduce(function(a,d){ return d.size_gb>(a?a.size_gb:0) ? d : a; }, null);
  chk('Диск', hw.disks.length ? hw.disks.map(function(d){ return d.model+' '+d.size_gb+' ГБ'+(d.media?' '+d.media:'')+(d.health&&d.health!=='Healthy'?' ['+d.health+']':''); }).join('; ') : 'не найден',
    e.diskGb ? (biggest ? near(biggest.size_gb, e.diskGb) : false) : (hw.disks.some(function(d){ return d.health && d.health!=='Healthy'; }) ? false : null), e.diskGb ? e.diskGb+' ГБ' : '');
  hw.gpus.forEach(function(g){ chk('Видео', g.name+(g.vram_mb?' · '+g.vram_mb+' МБ':'')+(g.driver_version?' · драйвер '+g.driver_version:''), null); });
  chk('Плата', hw.board || '—', null);
  chk('BIOS', (hw.bios_version||'—')+(hw.bios_date?' от '+hw.bios_date:''),
    e.biosContains ? (hw.bios_version||'').toLowerCase().indexOf(String(e.biosContains).toLowerCase())>=0 : null, e.biosContains);
  chk('Тип корпуса', hw.is_laptop ? 'ноутбук' : 'настольный ПК / другое', null);
  return { lines:lines, bad:bad };
}

/* ---------- реальные данные для runner-категорий ----------
   Каждая ветка возвращает { lines, verdict }: verdict — автооценка по порогам
   профиля ({status:'pass'|'fail'|'na', note}) или null, если оценить нельзя. */
function fetchCategory(kind){
  if (kind==='sys'){
    return invoke('get_hardware_summary').then(function(hw){
      S.hw = hw;
      var r = sysReport(hw);
      var hasExp = Object.keys(profile().expect||{}).length>0;
      return { lines:r.lines, verdict: r.bad.length
        ? { status:'fail', note:'Не совпадает с профилем «'+profile().name+'»: '+r.bad.join(', ') }
        : { status:'pass', note: hasExp ? 'Железо совпадает с профилем «'+profile().name+'»' : 'Сводка собрана (эталона в профиле нет)' } };
    });
  }
  if (kind==='usb'){
    return invoke('list_usb_devices').then(function(list){
      var bad = list.filter(function(d){ return d.status!=='OK'; });
      return {
        lines: list.length ? list.map(function(d){ return d.name + ' — ' + (d.status==='OK'?'работает':d.status); }) : ['USB-устройства не обнаружены (кроме встроенных корневых хабов).'],
        verdict: bad.length ? { status:'fail', note:'USB-устройства с ошибкой: '+bad.map(function(d){ return d.name; }).join(', ') }
                            : { status:'pass', note: list.length ? 'USB-устройства без ошибок ('+list.length+')' : 'Ошибок USB нет; порты проверьте флешкой' }
      };
    });
  }
  if (kind==='bt'){
    return invoke('list_bluetooth_devices').then(function(list){
      var bad = list.filter(function(d){ return d.status!=='OK'; });
      return {
        lines: list.length ? list.map(function(d){ return d.name + ' — ' + (d.status==='OK'?'работает':d.status); }) : ['Bluetooth-адаптер не обнаружен или отключён.'],
        verdict: !list.length ? absent('bt','Bluetooth-адаптер')
          : bad.length ? { status:'fail', note:'Bluetooth с ошибкой: '+bad.map(function(d){ return d.name; }).join(', ') }
          : { status:'pass', note:'Bluetooth-адаптер работает' }
      };
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
        var off = adapters.filter(function(a){ return a.status==='Disabled' || a.status==='Not Present'; });
        return { lines: lines.concat(networks.slice(0,8)),
          verdict: !adapters.length ? absent('wifi','Wi-Fi адаптер')
            : off.length ? { status:'fail', note:'Wi-Fi адаптер отключён или недоступен' }
            : !networks.length ? { status:'fail', note:'Адаптер есть, но сетей не видит' }
            : { status:'pass', note:'Wi-Fi работает, видимых сетей: '+networks.length } };
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
      return { lines:lines, verdict:verdict };
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
    { tag:'AUTO', title:'Автопрогон', desc:'Последовательная проверка по профилю модели: сверка железа, пороги батареи, автоматические вердикты.', meta:'профиль: '+profile().name+' · '+(profile().tests||[]).length+' тестов', badge:'НОВОЕ', hot:true, act:'echips.autoStart()' },
    { tag:'DIA', title:'Диагностика оборудования', desc:CATS.length+' категорий тестов, датчики (где доступны), стресс-тест и отчёт.', meta:CATS.length+' категорий · TXT / JSON', badge:'РУЧНОЙ', hot:false, go:'dash' },
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
      return '<div class="mode'+(m.hot?' is-new':'')+'" onclick="'+(m.act || "echips.go('"+m.go+"')")+'">'+
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
      '<button class="btn btn-primary" onclick="echips.autoStart()">Автопрогон</button>'+
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

/* Соответствие KeyboardEvent.code позициям клавиш в раскладке KEYROWS. */
var CODEMAP = (function(){
  var named = { 'Esc':['Escape'],'Del':['Delete'],'`':['Backquote'],'-':['Minus'],'=':['Equal'],'Bksp':['Backspace'],'Tab':['Tab'],
    '[':['BracketLeft'],']':['BracketRight'],'\\':['Backslash'],'Caps':['CapsLock'],';':['Semicolon'],"'":['Quote'],
    'Enter':['Enter','NumpadEnter'],',':['Comma'],'.':['Period'],'/':['Slash'],'Ctrl':['ControlLeft','ControlRight'],
    'Win':['MetaLeft','MetaRight'],'Space':['Space'],'←':['ArrowLeft'],'↑':['ArrowUp'],'↓':['ArrowDown'],'→':['ArrowRight'] };
  var map = {};
  KEYROWS.forEach(function(row,ri){
    row.forEach(function(label,ki){
      var id = ri+':'+ki, codes;
      if (label==='Shift') codes = [ki===0 ? 'ShiftLeft' : 'ShiftRight'];
      else if (label==='Alt') codes = [ki===3 ? 'AltLeft' : 'AltRight'];
      else if (named[label]) codes = named[label];
      else if (/^F\d+$/.test(label)) codes = [label];
      else if (/^\d$/.test(label)) codes = ['Digit'+label, 'Numpad'+label];
      else if (/^[A-Z]$/.test(label)) codes = ['Key'+label];
      else codes = [];
      codes.forEach(function(c){ map[c] = id; });
    });
  });
  return map;
})();
document.addEventListener('keydown', function(e){
  if (S.screen!=='test' || cat().kind!=='keyboard') return;
  if (e.target && (e.target.tagName==='INPUT' || e.target.tagName==='TEXTAREA')) return;
  e.preventDefault();
  var id = CODEMAP[e.code];
  if (id && !S.keys[id]){ S.keys[id] = true; render(); }
  else if (!id) { S.lastUnknown = e.code; }
});

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
    '<div class="kbnote">Клавиши подсвечиваются при нажатии на самой клавиатуре. Fn обычно не виден системе — отметьте её кликом мыши.'+(S.lastUnknown?' Не найдена в раскладке: '+esc(S.lastUnknown)+'.':'')+'</div></div>';
}
function paintFill(){
  var o = document.getElementById('fill-overlay'); if(!o) return;
  o.style.background = FILLS[S.fill].color;
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
    '<div class="log">'+body+'</div>'+
    (S.verdict && S.verdict.status ? '<div class="kbnote" style="margin-top:8px">Автооценка: '+({pass:'пройден',fail:'не пройден',na:'не применимо'}[S.verdict.status])+' — '+esc(S.verdict.note)+'</div>' : '')+'</div>';
}
function fieldCamera(){
  return '<div class="camwrap"><div class="preview" style="position:relative;overflow:hidden">'+
    (S.camStream
      ? '<video id="cam-preview" autoplay muted playsinline style="width:100%;height:100%;object-fit:cover;border-radius:10px"></video>'
      : '<div class="lens">CAM</div><div class="m">'+(S.runError?esc(S.runError):'запрос доступа к камере…')+'</div>')+
    '</div>'+
    '<div class="side">'+ CAMCHECKS.map(function(s){ return '<div class="note">'+s+'</div>'; }).join('') +'</div></div>';
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

function autoBanner(){
  var a = S.auto; if (!a.on) return '';
  var n = a.ids.length;
  var btns = '<button class="btn btn-ghost" onclick="echips.autoStop()">Прервать автопрогон</button>';
  if (a.stopped) btns = '<button class="btn btn-ghost" onclick="echips.autoNext()">Продолжить</button><button class="btn btn-primary" onclick="echips.autoReport()">К отчёту</button>';
  else if (a.waiting) btns = '<button class="btn btn-primary" onclick="echips.autoNext()">Далее</button>' + btns;
  return '<div class="autobar"><div class="ab-top"><span class="eyebrow">Автопрогон · профиль «'+esc(profile().name)+'»</span>'+
    '<span class="idx">шаг '+(a.idx+1)+' из '+n+'</span></div>'+
    '<div class="bar"><div class="fill" style="width:'+(a.idx/n*100).toFixed(0)+'%"></div></div>'+
    (a.msg ? '<div class="ab-msg '+a.cls+'">'+esc(a.msg)+'</div>' : '')+
    '<div class="headactions">'+btns+'</div></div>';
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
    autoBanner()+
    '<div class="testhead"><div><h2>'+c.name+'</h2><div class="hint">'+c.method+'</div></div>'+
    '<div class="base">'+c.tag+' · '+c.impl+'</div></div>'+
    '<div class="field">'+field+'</div>'+
    '<div class="verdict">'+
      '<input placeholder="Комментарий техника — попадёт в отчёт" value="'+esc(S.comments[c.id]||'')+'" oninput="echips.comment(this.value)">'+
      '<button class="btn btn-ghost" onclick="echips.mark(\'na\')" title="Такого узла нет в этой модели (например, тачпад на настольном ПК)">Не применимо</button>'+
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
  var viewKey = S.screen+'|'+S.cat+'|'+(S.screen==='drivers'?S.drv.step:'')+'|'+(S.screen==='mb'?S.mb.step:'');
  var isNewView = viewKey !== S.viewKey; S.viewKey = viewKey;
  host.innerHTML = S.screen==='start' ? screenStart()
    : S.screen==='drivers' ? screenDrivers()
    : S.screen==='mb' ? screenMb()
    : S.screen==='dash' ? screenDash()
    : S.screen==='test' ? screenTest()
    : S.screen==='sensors' ? screenSensors()
    : S.screen==='stress' ? screenStress() : screenReport();
  if (isNewView && host.firstElementChild) host.firstElementChild.classList.add('enter');
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
  if (S.tone!==null && S.screen==='test' && cat().kind==='audio'){
    // Обновляем только спектр: полная перерисовка каждый кадр ломала клики по кнопкам.
    var sp = document.querySelector('.spectrum');
    if (sp) sp.innerHTML = spectrumBars();
  }
})();

document.addEventListener('DOMContentLoaded', function(){
  loadDevice();
  render();
});
})();
