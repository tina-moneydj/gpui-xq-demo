const cp=require('child_process'),fs=require('fs'),puppeteer=require('/workspace/web/harness/node_modules/puppeteer-core');
(async()=>{
 const fh=cp.spawn(process.env.HOME+'/.dotnet/dotnet',['WryFeedHost.dll','--demo-ticks'],{cwd:'/workspace/web/feedhost/WryFeedHost/bin/Release/net8.0',env:{...process.env,XQ_FEED_PORT:'47741'},stdio:'ignore'});
 await new Promise(r=>setTimeout(r,2500));
 const br=cp.spawn('/workspace/web/gpui-xq-demo/target/release/xq-ws-bridge',['--listen','127.0.0.1:47760','--feed','127.0.0.1:47741','--www','/workspace/web/gpui-xq-demo/dist/web'],{stdio:'ignore'});
 await new Promise(r=>setTimeout(r,800));
 const b=await puppeteer.launch({executablePath:'/usr/bin/google-chrome',headless:false,defaultViewport:null,args:['--no-first-run','--window-size=1200,720','--app=about:blank','--user-data-dir=/tmp/xq-webgpu-try','--ignore-gpu-blocklist','--enable-unsafe-webgpu','--use-webgpu-adapter=swiftshader','--enable-features=Vulkan,WebGPUService','--use-angle=swiftshader'],env:{...process.env,DISPLAY:':7'}});
 const p=(await b.pages())[0]; const log=[]; p.on('console',m=>{const s=m.text(); if(!s.startsWith('[metrics]')) log.push(s.slice(0,300));});
 await p.goto('http://127.0.0.1:47760/index.html');
 const ad=await p.evaluate(async()=>{ if(!navigator.gpu) return 'no navigator.gpu'; const a=await navigator.gpu.requestAdapter(); if(!a) return 'requestAdapter null'; const i=a.info||{}; return JSON.stringify({vendor:i.vendor,arch:i.architecture,desc:i.description,fallback:a.isFallbackAdapter});});
 await new Promise(r=>setTimeout(r,12000));
 await p.screenshot({path:'/workspace/web/out/webgpu-try.png'});
 console.log('adapter:',ad); console.log(log.filter(l=>/WebGPU|WebGL|adapter|wgpu|blend|panic|error/i.test(l)).slice(0,15).join('\n'));
 await b.close(); fh.kill(); br.kill(); fs.rmSync('/tmp/xq-webgpu-try',{recursive:true,force:true});
})();
