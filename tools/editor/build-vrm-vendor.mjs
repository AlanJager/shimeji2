import {build,version as esbuildVersion} from 'esbuild';
import {readFile,writeFile,readdir,mkdir} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

process.chdir(fileURLToPath(new URL('.',import.meta.url)));
await mkdir('vendor',{recursive:true});
await build({entryPoints:['vrm-vendor-entry.mjs'],bundle:true,format:'esm',platform:'browser',target:'es2022',minify:true,legalComments:'external',outfile:'vendor/vrm-libs.mjs'});
const packages = {}, notices = [];
for (const name of ['three',...(await readdir('node_modules/@pixiv')).sort().map(name => '@pixiv/'+name)]) {
  const path = 'node_modules/'+name, info = JSON.parse(await readFile(path+'/package.json','utf8'));
  packages[name] = info.version;
  const license = (await readdir(path)).find(name => /^license(\.(txt|md))?$/i.test(name));
  if (!license) throw new Error('Missing license for '+name);
  notices.push(`${name} ${info.version}\n${await readFile(path+'/'+license,'utf8')}`);
}
await writeFile('vendor/LICENSES.txt',notices.join('\n\n'+'='.repeat(72)+'\n\n'));
const sha256 = createHash('sha256').update(await readFile('vendor/vrm-libs.mjs')).digest('hex');
await writeFile('vendor/manifest.json',JSON.stringify({packages,bundler:'esbuild '+esbuildVersion,sha256,source:'vrm-vendor-entry.mjs',rebuild:'npm ci --ignore-scripts && npm run build:vrm'},null,2)+'\n');
console.log('VRM libraries bundled locally:',sha256);
