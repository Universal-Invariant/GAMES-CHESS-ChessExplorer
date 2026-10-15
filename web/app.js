// Chess Visualizer frontend — plain ES module, no build step.
// Modular: themes / visualizers / api are independent registries.

const API = {
  async get(p){ const r = await fetch(p); return r.json(); },
  async post(p, body){ const r = await fetch(p, {method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body||{})}); return r.json(); }
};

// ---------------- FEN helpers ----------------
const FILES='abcdefgh';
function parseFen(fen){
  // returns {board: array of 64 (idx 0=a8 .. 63=h1), turn:'w'|'b'}
  const parts=fen.split(' ');
  const board=new Array(64).fill(null);
  let i=0;
  for(const ch of parts[0]){
    if(ch==='/'){continue;}
    if(/\d/.test(ch)){i+=+ch;}else{board[i]={p:ch};i++;}
  }
  return {board, turn:parts[1]||'w'};
}
const sqName=(idx)=>FILES[idx%8]+(7-((idx/8)|0));
const nameToIdx=(s)=>{const f=FILES.indexOf(s[0]);const r=+s[1];return (8-r)*8+f;};

// ---------------- THEMES (modular) ----------------
const GLYPH={K:'♔',Q:'♕',R:'♖',B:'♗',N:'♘',P:'♙'};
function glyph(piece){ const up=piece.p===piece.p.toUpperCase(); const g=GLYPH[piece.p.toUpperCase()]; return {g,white:up}; }
const THEMES={
  classic:{light:'#eeeed2',dark:'#769656',inkW:'#f8f8f8',inkB:'#111',outlineW:'#3a3a3a',outlineB:'#ddd',glow:false},
  dark:{light:'#3a3f4b',dark:'#232730',inkW:'#e8e8e8',inkB:'#0c0c0c',outlineW:'#666',outlineB:'#aaa',glow:false},
  neon:{light:'#12141a',dark:'#191c26',inkW:'#e0f7ff',inkB:'#0ff',outlineW:'#0ff',outlineB:'#f0f',glow:true},
};
function drawPiece(ctx,x,y,size,piece,theme,state){
  const {g,white}=glyph(piece);
  ctx.font=`${size*0.78}px serif`;
  ctx.textAlign='center';ctx.textBaseline='middle';
  if(theme.glow && state==='threat'){
    ctx.shadowColor='#f55';ctx.shadowBlur=18;
  } else if(theme.glow && state==='attacker'){
    ctx.shadowColor='#5af';ctx.shadowBlur=12;
  } else ctx.shadowBlur=0;
  ctx.fillStyle=white?theme.inkW:theme.inkB;
  ctx.fillText(g,x+size/2,y+size/2);
  if(theme.glow){ctx.strokeStyle=white?theme.outlineW:theme.outlineB;ctx.lineWidth=1;ctx.strokeText(g,x+size/2,y+size/2);}
  ctx.shadowBlur=0;
}

// ---------------- BOARD RENDERER ----------------
class BoardView{
  constructor(canvas){this.cv=canvas;this.ctx=canvas.getContext('2d');this.flip=false;this.theme=THEMES.classic;
    this.overlays=[]; // [{type:'arrow',from,to,color,label}]
    this.pieceStates={}; // idx -> 'threat'|'attacker'|null
    this.anim=null; // {piece, fromIdx,toIdx,t}
    this.onClick=null;
    canvas.addEventListener('click',e=>{
      if(!this.onClick)return;
      const rect=canvas.getBoundingClientRect();
      const px=(e.clientX-rect.left)*(canvas.width/rect.width);
      const py=(e.clientY-rect.top)*(canvas.height/rect.height);
      const size=canvas.width/8;
      let f=Math.floor(px/size),r=Math.floor(py/size);
      if(this.flip){f=7-f;r=7-r;}
      this.onClick(sqName(r*8+f));
    });
  }
  fenIdx(idx){return this.flip?63-idx:idx;}
  draw(fen){
    const {ctx,cv}=this;const size=cv.width/8;
    const st=parseFen(fen);
    for(let idx=0;idx<64;idx++){
      const d=this.fenIdx(idx);
      const x=(d%8)*size,y=((d/8)|0)*size;
      const light=(((idx/8)|0)+idx%8)%2===0;
      ctx.fillStyle=light?this.theme.light:this.theme.dark;
      ctx.fillRect(x,y,size,size);
    }
    // overlays under pieces: heat squares
    for(const o of this.overlays){
      if(o.type==='heat'){
        const d=this.fenIdx(nameToIdx(o.sq));
        const x=(d%8)*size,y=((d/8)|0)*size;
        ctx.fillStyle=o.color;ctx.fillRect(x,y,size,size);
      }
    }
    // pieces
    for(let idx=0;idx<64;idx++){
      const pc=st.board[idx];if(!pc)continue;
      if(this.anim && this.anim.fromIdx===idx)continue; // drawn in motion
      const d=this.fenIdx(idx);
      drawPiece(ctx,(d%8)*size,((d/8)|0)*size,size,pc,this.theme,this.pieceStates[idx]);
    }
    // animated piece on top
    if(this.anim){
      const a=this.anim;
      const fd=this.fenIdx(a.fromIdx),td=this.fenIdx(a.toIdx);
      const fx=(fd%8)*size+size/2,fy=((fd/8)|0)*size+size/2;
      const tx=(td%8)*size+size/2,ty=((td/8)|0)*size+size/2;
      const x=fx+(tx-fx)*a.t-size/2,y=fy+(ty-fy)*a.t-size/2;
      drawPiece(ctx,x,y,size,a.piece,this.theme,'attacker');
    }
    // arrows above everything
    for(const o of this.overlays){
      if(o.type==='arrow')this.arrow(o);
    }
  }
  arrow(o){
    const {ctx,cv}=this;const size=cv.width/8;
    const a=this.fenIdx(nameToIdx(o.from)),b=this.fenIdx(nameToIdx(o.to));
    const x1=(a%8)*size+size/2,y1=((a/8)|0)*size+size/2;
    const x2=(b%8)*size+size/2,y2=((b/8)|0)*size+size/2;
    ctx.strokeStyle=o.color||'#e33';ctx.fillStyle=o.color||'#e33';
    ctx.lineWidth=size*0.09;ctx.lineCap='round';
    ctx.beginPath();ctx.moveTo(x1,y1);ctx.lineTo(x2,y2);ctx.stroke();
    const ang=Math.atan2(y2-y1,x2-x1),hl=size*0.28;
    ctx.beginPath();ctx.moveTo(x2,y2);
    ctx.lineTo(x2-hl*Math.cos(ang-0.45),y2-hl*Math.sin(ang-0.45));
    ctx.lineTo(x2-hl*Math.cos(ang+0.45),y2-hl*Math.sin(ang+0.45));
    ctx.closePath();ctx.fill();
    if(o.label){ctx.font=`bold ${size*0.3}px sans-serif`;ctx.textAlign='center';ctx.fillStyle='#fff';
      ctx.fillText(o.label,(x1+x2)/2,(y1+y2)/2);}
  }
}

// ---------------- VISUALIZERS (modular playback styles) ----------------
// Each visualizer: play(boardView, frames, ply, speedMs, done) advances animation.
const VISUALIZERS={
  instant:{label:'Instant',run(bv,frames,i,speed,done){bv.anim=null;bv.draw(frames[i].fen);setTimeout(()=>done(),speed);}},
  walk:{label:'Walk',run(bv,frames,i,speed,done){
    const f=frames[i];const st=parseFen(f.before||frames[Math.max(0,i-1)].fen);
    const fromIdx=nameToIdx(f.uci.slice(0,2)),toIdx=nameToIdx(f.uci.slice(2,4));
    const piece=st.board[fromIdx]||{p:'P'};
    const dur=Math.min(speed,420);const t0=performance.now();
    (function step(now){
      const t=Math.min(1,(now-t0)/dur);
      bv.anim={piece,fromIdx,toIdx,t:ease(t)};
      bv.draw(i>0?frames[i-1].fen:f.before||f.fen);
      if(t<1)requestAnimationFrame(step);
      else{bv.anim=null;bv.draw(f.fen);setTimeout(()=>done(),Math.max(0,speed-dur));}
    })(t0);
  }},
  battle:{label:'Battle (overlapped)',run(bv,frames,i,speed,done){
    // start current move's animation and schedule next one at 60% overlap
    VISUALIZERS.walk.run(bv,frames,i,speed,()=>{});
    setTimeout(done,speed*0.6);
  }},
};
const ease=t=>t<0.5?2*t*t:1-Math.pow(-2*t+2,2)/2;

// feature overlays from analysis modules → arrows/heat/piece states
function applyFrameOverlays(bv,frame){
  bv.overlays=[];bv.pieceStates={};
  const feats=frame.features||[];
  for(const [name,val] of feats){
    if(name==='tactics'){
      for(const h of (val.hanging||[]))bv.pieceStates[nameToIdx(h.sq)]='threat';
      for(const a of (val.attacks||[]))bv.overlays.push({type:'arrow',from:a.from,to:a.to,color:'#e05c5c'});
      for(const p of (val.pins||[]))bv.overlays.push({type:'arrow',from:p.attacker,to:p.king,color:'#e5b567',label:'pin'});
    }
    if(name==='material'&&val.best_sq)bv.overlays.push({type:'heat',sq:val.best_sq,color:'rgba(80,160,255,.25)'});
  }
  if(frame.class==='blunder')bv.overlays.push({type:'arrow',from:frame.uci.slice(0,2),to:frame.uci.slice(2,4),color:'#ff2d55',label:'??'});
  if(frame.class==='good'&&frame.best_uci&&frame.best_uci!==frame.uci)
    bv.overlays.push({type:'arrow',from:frame.best_uci.slice(0,2),to:frame.best_uci.slice(2,4),color:'#3fb950',label:'best'});
}

// ---------------- PLAYTHROUGH ENGINE ----------------
class Playthrough{
  constructor(bv){this.bv=bv;this.frames=[];this.i=0;this.playing=false;this.speed=450;this.viz='walk';}
  load(frames){this.frames=frames;this.i=0;this.stop();if(frames.length)this.render(0);}
  render(i){
    if(!this.frames.length)return;
    this.i=Math.max(0,Math.min(i,this.frames.length-1));
    const f=this.frames[this.i];
    applyFrameOverlays(this.bv,f);
    this.bv.anim=null;
    this.bv.draw(f.fen);
    updatePtInfo(f,this.i,this.frames.length);
  }
  play(){
    if(this.playing||!this.frames.length)return;
    this.playing=true;
    const step=()=>{
      if(!this.playing)return;
      if(this.i>=this.frames.length-1){this.playing=false;return;}
      const next=this.i+1;
      VISUALIZERS[this.viz].run(this.bv,this.frames,next,this.speed,()=>{
        this.i=next;applyFrameOverlays(this.bv,this.frames[next]);updatePtInfo(this.frames[next],next,this.frames.length);
        if(this.playing)setTimeout(step,0);
      });
    };
    step();
  }
  stop(){this.playing=false;}
  fwd(){this.stop();VISUALIZERS[this.viz].run(this.bv,this.frames,Math.min(this.i+1,this.frames.length-1),this.speed,()=>{this.i=Math.min(this.i+1,this.frames.length-1);applyFrameOverlays(this.bv,this.frames[this.i]);updatePtInfo(this.frames[this.i],this.i,this.frames.length);});}
}
function updatePtInfo(f,i,n){
  const el=document.getElementById('pt-info');
  const cls=f.class||'';
  el.innerHTML=`ply ${i+1}/${n} · <b>${f.san}</b> · eval ${(f.eval_cp/100).toFixed(2)} <span class="${cls}">${cls}</span>`;
}

// ---------------- VIEWS / WIRING ----------------
const ptBoard=new BoardView(document.getElementById('pt-board'));
const playBoard=new BoardView(document.getElementById('play-board'));
const exBoard=new BoardView(document.getElementById('ex-board'));
const pt=new Playthrough(ptBoard);

// tabs
document.querySelectorAll('.tab').forEach(b=>b.onclick=()=>{
  document.querySelectorAll('.tab').forEach(x=>x.classList.remove('active'));
  document.querySelectorAll('.view').forEach(x=>x.classList.remove('active'));
  b.classList.add('active');
  document.getElementById('view-'+b.dataset.view).classList.add('active');
});

// transport controls
document.getElementById('tp-play').onclick=e=>{pt.playing?(pt.stop(),e.target.textContent='▶ Play'):(e.target.textContent='⏸ Pause',pt.play());};
document.getElementById('tp-next').onclick=()=>pt.render(pt.i+1);
document.getElementById('tp-prev').onclick=()=>pt.render(0);
document.getElementById('tp-back').onclick=()=>pt.render(pt.i-1);
document.getElementById('tp-fwd').onclick=()=>pt.fwd();
document.getElementById('tp-speed').oninput=e=>pt.speed=1240-e.target.valueAsNumber; // slider right = faster
document.getElementById('tp-viz').onchange=e=>pt.viz=e.target.value;
document.getElementById('tp-theme').onchange=e=>{ptBoard.theme=THEMES[e.target.value];if(pt.frames.length)ptBoard.draw(pt.frames[pt.i].fen);};

// import & query
document.getElementById('import-btn').onclick=async()=>{
  const path=document.getElementById('import-path').value.trim();
  if(!path)return;
  const btn=document.getElementById('import-btn');btn.disabled=true;btn.textContent='Importing…';
  const r=await API.post('/api/import',{path});
  btn.disabled=false;btn.textContent='Import';
  alert(JSON.stringify(r));refreshStatus();queryGames();
};
document.getElementById('query-btn').onclick=()=>queryGames();
document.getElementById('shuffle-load').onclick=()=>{document.getElementById('filter-expr').value='';queryGames(true);};
async function queryGames(shuffle){
  const p=new URLSearchParams();
  p.set('filter',document.getElementById('filter-expr').value||'');
  p.set('sort',shuffle?'score':document.getElementById('sort-field').value);
  p.set('desc',shuffle?'1':(document.getElementById('sort-desc').checked?'1':'0'));
  p.set('limit','60');
  const r=await API.get('/api/games?'+p.toString());
  const list=document.getElementById('game-list');list.innerHTML='';
  (r.games||[]).forEach(g=>{
    const d=document.createElement('div');d.className='game-item';
    d.innerHTML=`<span>#${g.id} ${esc(g.white)}–${esc(g.black)}</span><span class="meta">${esc(g.eco||'')} ${g.white_elo|0}/${g.black_elo|0}</span>`;
    d.onclick=()=>{document.querySelectorAll('.game-item.sel').forEach(x=>x.classList.remove('sel'));d.classList.add('sel');loadGame(g.id);};
    list.appendChild(d);
  });
  if(!r.games||!r.games.length)list.innerHTML='<div class="hint">No games found. Import a PGN first.</div>';
}
let curGame=null,curFrames=[];
async function loadGame(id){
  const g=await API.get('/api/game?id='+id);curGame=g;
  const engine=localStorage.getItem('engineOn')!=='0';
  const fr=await API.get(`/api/frames?id=${id}&engine=${engine?1:0}&ms=120`);
  curFrames=fr.frames||[];
  pt.load(curFrames);
  const box=document.getElementById('analysis-box');
  box.textContent=curFrames.length?`Loaded ${curFrames.length} analysed plies (engine: ${fr.engine}).`:'no frames';
}
const esc=s=>String(s??'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));

// AI
document.getElementById('ai-analyze').onclick=async()=>{
  if(!curGame)return alert('Load a game first');
  const box=document.getElementById('analysis-box');box.textContent='AI thinking…';
  const r=await API.post('/api/ai',{game_id:curGame.id,analyze:true});
  box.textContent=r.analysis||r.error||'?';
};
document.getElementById('ai-ask').onclick=async()=>{
  const q=document.getElementById('ai-question').value.trim();if(!q)return;
  const fen=pt.frames.length?pt.frames[pt.i].fen:'startpos';
  const box=document.getElementById('analysis-box');box.textContent='AI thinking…';
  const r=await API.post('/api/ai',{question:q,fen});
  box.textContent=r.answer||r.error||'?';
};

// ---------------- OPENING EXPLORER ----------------
let exSans=[];
const exState={fen: 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1'};
async function refreshExplorer(){
  const r=await API.get('/api/openings?prefix='+encodeURIComponent(exSans.join(' ')));
  const cur=document.getElementById('ex-current');
  cur.textContent=r.current?`${r.current.eco} ${r.current.name}`:(exSans.length?'(no named line)':'Start position');
  const bl=document.getElementById('ex-branches');bl.innerHTML='';
  (r.branches||[]).forEach(b=>{
    const d=document.createElement('div');d.className='branch-item';
    d.innerHTML=`<span>${esc(b.san)}</span><span class="meta">${esc(b.eco||'')} ${esc(b.name||'')}</span>`;
    d.onclick=async()=>{
      exSans.push(b.san);
      const lg=await API.post('/api/legals',{fen:exState.fen});
      const m=(lg.moves||[]).find(x=>x.san===b.san);
      if(m){const r=await API.post('/api/playmove',{fen:exState.fen,move:m.uci,mode:'none'});if(r.fen)exState.fen=r.fen;}
      exBoard.draw(exState.fen);refreshExplorer();
    };
    bl.appendChild(d);
  });
  const st=document.getElementById('ex-stats');st.innerHTML='';
  (r.db_openings||[]).slice(0,15).forEach(o=>{
    const d=document.createElement('div');d.className='branch-item';
    d.innerHTML=`<span>${esc(o.eco)} ${esc(o.name)}</span><span class="meta">${o.games}</span>`;
    st.appendChild(d);
  });
}
function toUciGuess(){return '';}
document.getElementById('ex-reset').onclick=()=>{exSans=[];exState.fen='rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';exBoard.draw(exState.fen);refreshExplorer();};

// ---------------- LOCAL PLAY ----------------
const play={fen:'startpos',history:[],flipped:false};
async function playRefresh(){
  const lg=await API.post('/api/legals',{fen:play.fen});
  play.info=lg;
  document.getElementById('play-info').textContent=lg.over?('Game over: '+JSON.stringify(lg.over)):(parseFen(play.fen).turn==='w'?'White to move':'Black to move');
}
let selSq=null;
playBoard.onClick=async(sq)=>{
  const st=parseFen(play.fen);
  const idx=nameToIdx(sq);
  if(selSq){
    const promo=/[qrbn]/.test(sq)?sq[4]:'';
    const uci=selSq+sq+promo;
    const mv=(play.info?.moves||[]).find(m=>m.uci===uci||(m.uci.length===4&&uci.length===5&&m.uci===uci.slice(0,4)));
    selSq=null;
    if(mv){
      const r=await API.post('/api/playmove',{fen:play.fen,move:mv.uci,mode:document.getElementById('play-mode').value,ms:400});
      play.fen=r.fen;play.history.push(mv.san);
      if(r.reply)play.history.push(r.reply_san||r.reply);
      renderPlayMoves();playRefresh();
      return;
    }
  }
  if(st.board[idx])selSq=sq;else selSq=null;
};
function renderPlayMoves(){
  const el=document.getElementById('play-moves');
  let out='';for(let i=0;i<play.history.length;i+=2)out+=`${i/2+1}. ${play.history[i]} ${play.history[i+1]||''}  `;
  el.textContent=out;el.scrollTop=el.scrollHeight;
}
document.getElementById('play-new').onclick=()=>{play.fen='startpos';play.history=[];renderPlayMoves();playRefresh();playBoard.draw(play.fen);};
document.getElementById('play-flip').onclick=()=>{playBoard.flip=!playBoard.flip;playBoard.draw(play.fen);};

// ---------------- SETTINGS ----------------
document.getElementById('cfg-save').onclick=async()=>{
  const body={ai:{base_url:document.getElementById('cfg-ai-url').value,api_key:document.getElementById('cfg-ai-key').value},engine_binary:document.getElementById('cfg-engine').value};
  const model=document.getElementById('cfg-ai-model').value;
  if(model)body.ai.model=model;
  const r=await API.post('/api/config',body);
  document.getElementById('cfg-msg').textContent=r.ok?'saved ✓':JSON.stringify(r);
};
async function loadSettings(){const c=await API.get('/api/config');
  document.getElementById('cfg-ai-url').value=c.ai?.base_url||'';
  document.getElementById('cfg-ai-model').value=c.ai?.model||'';
  document.getElementById('cfg-engine').value=c.engine_binary||'';
}

// ---------------- STATUS ----------------
async function refreshStatus(){const s=await API.get('/api/status');
  document.getElementById('status-indicator').textContent=`${s.games} games · ${s.openings} openings · engine: ${s.engine_available}`;
}

// boot
ptBoard.draw('rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1');
playBoard.draw(play.fen);
exBoard.draw(exState.fen);
refreshStatus();refreshExplorer();loadSettings();
queryGames();
