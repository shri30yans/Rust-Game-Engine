// Original "player" character built from primitives. Stand-in for an authored glTF.
import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';

function plateTexture(text, bg = '#F3F5F8', fg = '#0C0F14') {
  const c = document.createElement('canvas'); c.width = 256; c.height = 160;
  const g = c.getContext('2d');
  g.fillStyle = bg; g.fillRect(0, 0, 256, 160);
  g.fillStyle = fg; g.font = '900 128px "Big Shoulders Display", Impact, sans-serif';
  g.textAlign = 'center'; g.textBaseline = 'middle'; g.fillText(text, 128, 86);
  const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace; t.anisotropy = 8;
  return t;
}

function quiltTexture(base) {
  const c = document.createElement('canvas'); c.width = 256; c.height = 256;
  const g = c.getContext('2d');
  g.fillStyle = base; g.fillRect(0, 0, 256, 256);
  for (let y = 0; y < 256; y += 42) {
    const grd = g.createLinearGradient(0, y, 0, y + 42);
    grd.addColorStop(0, 'rgba(255,255,255,.10)'); grd.addColorStop(.5, 'rgba(255,255,255,0)'); grd.addColorStop(1, 'rgba(0,0,0,.35)');
    g.fillStyle = grd; g.fillRect(0, y, 256, 42);
  }
  const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace;
  t.wrapS = t.wrapT = THREE.RepeatWrapping; return t;
}

/**
 * opts: team (hex), number (string), pose: 'idle' | 'aim' | 'hero'
 */
export function makePlayer(opts = {}) {
  const team = new THREE.Color(opts.team ?? 0xff7a3d);
  const root = new THREE.Group();
  const shadow = (m) => { m.castShadow = true; m.receiveShadow = true; return m; };

  const M = {
    jacket: new THREE.MeshStandardMaterial({ map: quiltTexture('#3b4350'), roughness: .78 }),
    pants: new THREE.MeshStandardMaterial({ color: 0x23272f, roughness: .9 }),
    rig: new THREE.MeshStandardMaterial({ color: 0x15181d, roughness: .6, metalness: .2 }),
    pad: new THREE.MeshStandardMaterial({ color: 0x2c323c, roughness: .55 }),
    team: new THREE.MeshStandardMaterial({ color: team, roughness: .45 }),
    helmet: new THREE.MeshPhysicalMaterial({ color: 0xe9ecf0, roughness: .28, clearcoat: 1, clearcoatRoughness: .15 }),
    visor: new THREE.MeshPhysicalMaterial({ color: 0x0d2a36, metalness: .6, roughness: .08, clearcoat: 1, emissive: 0x0a3a4a, emissiveIntensity: .6 }),
    signal: new THREE.MeshStandardMaterial({ color: 0xff6a1a, roughness: .4, emissive: 0xff6a1a, emissiveIntensity: .15 }),
    glove: new THREE.MeshStandardMaterial({ color: 0x111318, roughness: .7 }),
    gun: new THREE.MeshStandardMaterial({ color: 0x1a1d22, roughness: .45, metalness: .5 }),
  };

  // legs
  for (const s of [-1, 1]) {
    const leg = new THREE.Group(); leg.position.set(.11 * s, .86, 0);
    const thigh = shadow(new THREE.Mesh(new THREE.CapsuleGeometry(.095, .32, 6, 14), M.pants)); thigh.position.y = -.2;
    const shin = shadow(new THREE.Mesh(new THREE.CapsuleGeometry(.08, .32, 6, 14), M.pants)); shin.position.y = -.56;
    const knee = shadow(new THREE.Mesh(new RoundedBoxGeometry(.15, .13, .08, 3, .03), M.pad)); knee.position.set(0, -.38, .07);
    const boot = shadow(new THREE.Mesh(new RoundedBoxGeometry(.15, .13, .3, 3, .04), M.rig)); boot.position.set(0, -.8, .05);
    leg.add(thigh, shin, knee, boot);
    if (opts.pose === 'aim') leg.rotation.z = .07 * s;
    else { leg.rotation.z = (s > 0 ? .09 : .02) * s; leg.rotation.x = s > 0 ? -.08 : .06; }
    root.add(leg);
  }
  const pelvis = shadow(new THREE.Mesh(new RoundedBoxGeometry(.36, .2, .24, 3, .05), M.rig)); pelvis.position.y = .9; root.add(pelvis);

  // torso: padded jacket
  const torso = new THREE.Group(); torso.position.y = 1.2; root.add(torso);
  const jacket = shadow(new THREE.Mesh(new THREE.CapsuleGeometry(.19, .3, 8, 24), M.jacket));
  jacket.scale.set(1.08, 1, .78);
  const hem = shadow(new THREE.Mesh(new THREE.CylinderGeometry(.2, .19, .12, 24), M.jacket)); hem.scale.set(1.05, 1, .8); hem.position.y = -.27; torso.add(hem);
  const hood = shadow(new THREE.Mesh(new THREE.TorusGeometry(.12, .045, 10, 24), M.jacket)); hood.rotation.x = Math.PI / 2; hood.position.set(0, .29, -.02); torso.add(hood); torso.add(jacket);
  const vest = shadow(new THREE.Mesh(new RoundedBoxGeometry(.32, .34, .07, 3, .03), M.rig)); vest.position.set(0, -.02, .14); torso.add(vest);
  for (let i = -1; i <= 1; i++) {
    const pouch = shadow(new THREE.Mesh(new RoundedBoxGeometry(.09, .12, .05, 2, .015), M.pad)); pouch.position.set(i * .1, -.12, .185); torso.add(pouch);
  }
  const plate = new THREE.Mesh(new THREE.PlaneGeometry(.17, .105), new THREE.MeshStandardMaterial({ map: plateTexture(opts.number ?? '07'), roughness: .5 }));
  plate.position.set(0, .08, .177); torso.add(plate);
  const pack = shadow(new THREE.Mesh(new RoundedBoxGeometry(.32, .4, .16, 3, .04), M.rig)); pack.position.set(0, .02, -.2); torso.add(pack);
  const packStripe = new THREE.Mesh(new THREE.BoxGeometry(.04, .38, .02), M.team); packStripe.position.set(.12, .02, -.285); torso.add(packStripe);
  const collar = shadow(new THREE.Mesh(new THREE.CylinderGeometry(.1, .13, .1, 20), M.rig)); collar.position.y = .3; torso.add(collar);

  // arms with shoulder pivots
  const arms = [];
  for (const s of [-1, 1]) {
    const sh = new THREE.Group(); sh.position.set(.25 * s, .19, 0); torso.add(sh);
    const pad = shadow(new THREE.Mesh(new THREE.SphereGeometry(.085, 20, 14), M.team)); pad.scale.set(1.15, .75, 1.05); sh.add(pad);
    const upper = shadow(new THREE.Mesh(new THREE.CapsuleGeometry(.058, .2, 6, 14), M.jacket)); upper.position.y = -.17; sh.add(upper);
    const elbow = new THREE.Group(); elbow.position.y = -.33; sh.add(elbow);
    const fore = shadow(new THREE.Mesh(new THREE.CapsuleGeometry(.05, .2, 6, 14), M.pad)); fore.position.y = -.14; elbow.add(fore);
    const glove = shadow(new THREE.Mesh(new THREE.SphereGeometry(.055, 16, 12), M.glove)); glove.scale.set(.9, 1.2, .8); glove.position.y = -.29; elbow.add(glove);
    arms.push({ sh, elbow, s });
  }
  const pose = opts.pose ?? 'idle';
  for (const a of arms) {
    if (pose === 'idle') { a.sh.rotation.z = .06 * a.s; a.sh.rotation.x = .05; a.elbow.rotation.x = -.25; }
    if (pose === 'hero') { a.sh.rotation.z = a.s > 0 ? .5 : .05 * a.s; a.sh.rotation.x = a.s > 0 ? .1 : -.1; a.elbow.rotation.x = a.s > 0 ? -1.6 : -.3; a.elbow.rotation.z = a.s > 0 ? -.4 : 0; }
    if (pose === 'aim') { a.sh.rotation.x = -1.25; a.sh.rotation.z = a.s > 0 ? -.35 : .2; a.elbow.rotation.x = -.35; }
  }
  if (pose === 'aim') {
    // carbine held across the body
    const gun = new THREE.Group();
    const body = shadow(new THREE.Mesh(new RoundedBoxGeometry(.07, .11, .52, 2, .015), M.gun));
    const barrel = shadow(new THREE.Mesh(new THREE.CylinderGeometry(.015, .015, .26, 10), M.gun)); barrel.rotation.x = Math.PI / 2; barrel.position.z = .38;
    const mag = shadow(new THREE.Mesh(new RoundedBoxGeometry(.05, .16, .08, 2, .01), M.gun)); mag.position.set(0, -.12, .06); mag.rotation.x = .2;
    const stripe = new THREE.Mesh(new THREE.BoxGeometry(.072, .02, .3), M.signal); stripe.position.set(0, .03, .02);
    gun.add(body, barrel, mag, stripe);
    if (pose === 'aim') { gun.position.set(.08, .02, .4); }
    else { gun.position.set(0, -.18, .28); gun.rotation.set(.1, .0, -.9); }
    torso.add(gun);
  }

  // head + helmet
  const head = new THREE.Group(); head.position.y = 1.64; root.add(head);
  const shell = shadow(new THREE.Mesh(new THREE.SphereGeometry(.145, 40, 28), M.helmet)); shell.scale.set(1, 1.08, 1.08); head.add(shell);
  // visor: a band of a slightly larger sphere, centred on +z (phi = pi/2 faces +z)
  const visor = new THREE.Mesh(new THREE.SphereGeometry(.151, 40, 16, Math.PI / 2 - Math.PI * .3, Math.PI * .6, Math.PI * .36, Math.PI * .24), M.visor);
  visor.scale.set(1.02, 1.1, 1.08); head.add(visor);
  const stripe = new THREE.Mesh(new THREE.TorusGeometry(.158, .014, 8, 40, Math.PI * .6), M.signal);
  stripe.rotation.set(0, Math.PI / 2, 0); stripe.scale.set(1.08, 1.08, 1); head.add(stripe);
  const ear = shadow(new THREE.Mesh(new THREE.CylinderGeometry(.04, .04, .05, 16), M.rig)); ear.rotation.z = Math.PI / 2; ear.position.set(.148, -.01, 0); head.add(ear);
  const tally = new THREE.Mesh(new THREE.SphereGeometry(.012, 8, 8), new THREE.MeshBasicMaterial({ color: 0xff3020 })); tally.position.set(.175, .02, .02); head.add(tally);
  if (pose === 'aim') head.rotation.x = .05;
  if (pose === 'hero') { head.rotation.y = -.25; torso.rotation.y = .08; pelvis.rotation.y = -.06; }
  root.userData.headY = 1.95;
  return root;
}
