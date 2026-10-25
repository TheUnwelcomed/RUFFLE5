// SPDX-License-Identifier: GPL-3.0-or-later
// Replays a softgl call trace (trace.rs: one JSON array per line) in the browser's WebGL2 with
// the original GLSL, and returns the RGBA of the texture the "result" line names, rows as GL
// keeps them (row 0 = y 0), base64.
async function replayTrace(url, opts) {
  opts = opts || {};
  const text = await (await fetch(url)).text();
  const lines = text.split('\n');
  const log = [];
  let canvas = null, gl = null;
  let M = null;
  const fresh = () => ({ buf: {}, tex: {}, fb: {}, rb: {}, vao: {}, sh: {}, prog: {}, loc: {} });
  const b64 = (o) => {
    if (o === null) return null;
    const s = atob(o.b64);
    const a = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
    return a;
  };
  const loc = (p, name) => {
    const k = p + '\u0000' + name;
    if (!(k in M.loc)) M.loc[k] = gl.getUniformLocation(M.prog[p], name);
    return M.loc[k];
  };
  const obj = (map, id) => (id === null ? null : map[id]);
  let frame = 0, errors = 0, calls = 0, draws = 0, skipped = 0;
  let result = null;
  // The traced frame and its result texture (the last lines).
  let tFrame = 0, tTex = 0, tW = 0, tH = 0;
  for (let k = lines.length - 1; k >= 0; k--) {
    if (lines[k].startsWith('["result"')) { const r = JSON.parse(lines[k]); tTex = r[1]; tW = r[2]; tH = r[3]; }
    if (lines[k].startsWith('["frame"')) { tFrame = JSON.parse(lines[k])[1]; break; }
  }
  let owner = null, readFb = null, before = null, after = null;
  const snap = (into) => {
    const prevRead = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);
    if (!readFb) readFb = gl.createFramebuffer();
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, readFb);
    gl.framebufferTexture2D(gl.READ_FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, M.tex[tTex], 0);
    gl.readPixels(0, 0, tW, tH, gl.RGBA, gl.UNSIGNED_BYTE, into);
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, prevRead);
  };
  const t0 = performance.now();
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    if (!line) continue;
    const a = JSON.parse(line);
    const f = a[0];
    calls++;
    switch (f) {
      case 'context': {
        canvas = document.createElement('canvas');
        canvas.width = a[1]; canvas.height = a[2];
        gl = canvas.getContext('webgl2', { antialias: false, alpha: true });
        M = fresh();
        log.push('context ' + gl.getParameter(gl.VERSION) + ' | ' + gl.getParameter(gl.SHADING_LANGUAGE_VERSION));
        break;
      }
      case 'frame': frame = a[1]; break;
      case 'result': {
        const [_, tex, w, h] = a;
        const fb = gl.createFramebuffer();
        gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, M.tex[tex], 0);
        const st = gl.checkFramebufferStatus(gl.FRAMEBUFFER);
        const px = new Uint8Array(w * h * 4);
        gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
        let s = '';
        const CH = 0x8000;
        for (let i = 0; i < px.length; i += CH) s += String.fromCharCode.apply(null, px.subarray(i, i + CH));
        result = { w, h, frame, status: st, rgba: btoa(s) };
        if (owner) {
          const ob = new Uint8Array(owner.buffer);
          let os = '';
          for (let i = 0; i < ob.length; i += CH) os += String.fromCharCode.apply(null, ob.subarray(i, i + CH));
          result.owner = btoa(os);
        }
        li = lines.length;
        break;
      }
      case 'createBuffer': M.buf[a[1]] = gl.createBuffer(); break;
      case 'deleteBuffer': gl.deleteBuffer(M.buf[a[1]]); delete M.buf[a[1]]; break;
      case 'createTexture': M.tex[a[1]] = gl.createTexture(); break;
      case 'deleteTexture': gl.deleteTexture(M.tex[a[1]]); delete M.tex[a[1]]; break;
      case 'createFramebuffer': M.fb[a[1]] = gl.createFramebuffer(); break;
      case 'deleteFramebuffer': gl.deleteFramebuffer(M.fb[a[1]]); delete M.fb[a[1]]; break;
      case 'createRenderbuffer': M.rb[a[1]] = gl.createRenderbuffer(); break;
      case 'deleteRenderbuffer': gl.deleteRenderbuffer(M.rb[a[1]]); delete M.rb[a[1]]; break;
      case 'createVertexArray': M.vao[a[1]] = gl.createVertexArray(); break;
      case 'deleteVertexArray': gl.deleteVertexArray(M.vao[a[1]]); delete M.vao[a[1]]; break;
      case 'createShader': M.sh[a[2]] = gl.createShader(a[1]); break;
      case 'deleteShader': gl.deleteShader(M.sh[a[1]]); delete M.sh[a[1]]; break;
      case 'shaderSource': gl.shaderSource(M.sh[a[1]], a[2]); break;
      case 'compileShader': {
        gl.compileShader(M.sh[a[1]]);
        if (!gl.getShaderParameter(M.sh[a[1]], gl.COMPILE_STATUS)) log.push('compile ' + a[1] + ': ' + gl.getShaderInfoLog(M.sh[a[1]]));
        break;
      }
      case 'createProgram': M.prog[a[1]] = gl.createProgram(); break;
      case 'deleteProgram': gl.deleteProgram(M.prog[a[1]]); delete M.prog[a[1]]; break;
      case 'attachShader': gl.attachShader(M.prog[a[1]], M.sh[a[2]]); break;
      case 'linkProgram': {
        gl.linkProgram(M.prog[a[1]]);
        for (const k of Object.keys(M.loc)) if (k.startsWith(a[1] + '\u0000')) delete M.loc[k];
        if (!gl.getProgramParameter(M.prog[a[1]], gl.LINK_STATUS)) log.push('link ' + a[1] + ': ' + gl.getProgramInfoLog(M.prog[a[1]]));
        break;
      }
      case 'useProgram': gl.useProgram(obj(M.prog, a[1])); break;
      case 'uniform1i': gl.uniform1i(loc(a[1], a[2]), a[3]); break;
      case 'uniform1iv': gl.uniform1iv(loc(a[1], a[2]), new Int32Array(a[3])); break;
      case 'uniform1f': gl.uniform1f(loc(a[1], a[2]), a[3]); break;
      case 'uniform2f': gl.uniform2f(loc(a[1], a[2]), a[3], a[4]); break;
      case 'uniform3f': gl.uniform3f(loc(a[1], a[2]), a[3], a[4], a[5]); break;
      case 'uniform4f': gl.uniform4f(loc(a[1], a[2]), a[3], a[4], a[5], a[6]); break;
      case 'uniform1fv': gl.uniform1fv(loc(a[1], a[2]), new Float32Array(a[3])); break;
      case 'uniform3fv': gl.uniform3fv(loc(a[1], a[2]), new Float32Array(a[3])); break;
      case 'uniform4fv': gl.uniform4fv(loc(a[1], a[2]), new Float32Array(a[3])); break;
      case 'uniformMatrix4fv': gl.uniformMatrix4fv(loc(a[1], a[2]), a[3], new Float32Array(a[4])); break;
      case 'bindBuffer': gl.bindBuffer(a[1], obj(M.buf, a[2])); break;
      case 'bufferData': gl.bufferData(a[1], b64(a[2]), a[3]); break;
      case 'bindVertexArray': gl.bindVertexArray(obj(M.vao, a[1])); break;
      case 'enableVertexAttribArray': gl.enableVertexAttribArray(a[1]); break;
      case 'disableVertexAttribArray': gl.disableVertexAttribArray(a[1]); break;
      case 'vertexAttribPointer': gl.vertexAttribPointer(a[1], a[2], a[3], a[4], a[5], a[6]); break;
      case 'activeTexture': gl.activeTexture(a[1]); break;
      case 'bindTexture': gl.bindTexture(a[1], obj(M.tex, a[2])); break;
      case 'texParameteri': gl.texParameteri(a[1], a[2], a[3]); break;
      case 'pixelStorei': gl.pixelStorei(a[1], a[2]); break;
      case 'texImage2D': gl.texImage2D(a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], b64(a[9])); break;
      case 'generateMipmap': gl.generateMipmap(a[1]); break;
      case 'copyTexSubImage2D': gl.copyTexSubImage2D(a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]); break;
      case 'bindFramebuffer': gl.bindFramebuffer(a[1], obj(M.fb, a[2])); break;
      case 'framebufferTexture2D': gl.framebufferTexture2D(a[1], a[2], a[3], obj(M.tex, a[4]), a[5]); break;
      case 'bindRenderbuffer': gl.bindRenderbuffer(a[1], obj(M.rb, a[2])); break;
      case 'renderbufferStorage': gl.renderbufferStorage(a[1], a[2], a[3], a[4]); break;
      case 'framebufferRenderbuffer': gl.framebufferRenderbuffer(a[1], a[2], a[3], obj(M.rb, a[4])); break;
      case 'blitFramebuffer': gl.blitFramebuffer(a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10]); break;
      case 'enable': gl.enable(a[1]); break;
      case 'disable': gl.disable(a[1]); break;
      case 'viewport': gl.viewport(a[1], a[2], a[3], a[4]); break;
      case 'scissor': gl.scissor(a[1], a[2], a[3], a[4]); break;
      case 'clearColor': gl.clearColor(a[1], a[2], a[3], a[4]); break;
      case 'clearDepth': gl.clearDepth(a[1]); break;
      case 'clear': gl.clear(a[1]); break;
      case 'depthFunc': gl.depthFunc(a[1]); break;
      case 'depthMask': gl.depthMask(a[1]); break;
      case 'colorMask': gl.colorMask(a[1], a[2], a[3], a[4]); break;
      case 'cullFace': gl.cullFace(a[1]); break;
      case 'frontFace': gl.frontFace(a[1]); break;
      case 'polygonOffset': gl.polygonOffset(a[1], a[2]); break;
      case 'blendFunc': gl.blendFunc(a[1], a[2]); break;
      case 'blendFuncSeparate': gl.blendFuncSeparate(a[1], a[2], a[3], a[4]); break;
      case 'blendEquation': gl.blendEquation(a[1]); break;
      case 'blendEquationSeparate': gl.blendEquationSeparate(a[1], a[2]); break;
      case 'drawArrays':
      case 'drawElements': {
        if (opts.skipBefore && frame < opts.skipBefore) { skipped++; break; }
        const inT = frame === tFrame;
        if (inT && opts.stopAfter !== undefined && opts.stopAfter !== null && draws >= opts.stopAfter) { draws++; break; }
        if (inT && opts.owner) {
          if (!owner) { owner = new Int32Array(tW * tH).fill(-1); before = new Uint8Array(tW * tH * 4); after = new Uint8Array(tW * tH * 4); }
          snap(before);
        }
        if (f === 'drawArrays') gl.drawArrays(a[1], a[2], a[3]);
        else gl.drawElements(a[1], a[2], a[3], a[4]);
        if (inT && opts.owner) {
          snap(after);
          for (let k = 0, n = tW * tH; k < n; k++) {
            const o = k * 4;
            if (before[o] !== after[o] || before[o + 1] !== after[o + 1] || before[o + 2] !== after[o + 2] || before[o + 3] !== after[o + 3]) owner[k] = draws;
          }
        }
        if (inT) draws++;
        break;
      }
      default:
        log.push('unknown call ' + f);
    }
    if (gl && errors < 20 && (calls % 64 === 0 || f.startsWith('draw') || f === 'texImage2D' || f === 'linkProgram')) {
      const e = gl.getError();
      if (e !== gl.NO_ERROR) { errors++; log.push('GL error 0x' + e.toString(16) + ' at line ' + (li + 1) + ' (' + f + ', frame ' + frame + ')'); }
    }
  }
  log.push('calls ' + calls + ', target-frame draws ' + draws + ', skipped draws ' + skipped + ', ' + Math.round(performance.now() - t0) + ' ms');
  return { log, result };
}
