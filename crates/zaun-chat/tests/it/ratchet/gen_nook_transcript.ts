// Regenerates nook_transcript.json: `tsx gen_nook_transcript.ts <out.json> <path to nook checkout>` from inside the checkout.
import { readFileSync, writeFileSync } from "node:fs"
const nook = process.argv[3] // a checkout of nook
const {
  RatchetSession, currentE2EEncryptVersion, supportedE2EEncryptVRange, generateRcvE2EParams, generateSndE2EParams,
  encodeE2ERatchetParams, decodeE2ERatchetParams, pqX3dhRcv, pqX3dhSnd, initSndRatchet, initRcvRatchet, generateX448KeyPair,
  loadSntrup761, replyKEM_, x448PublicFromPrivate, paramsToUri, uriToParams, encodeE2ERatchetParamsUri, e2eParamsUriToString,
} = await import(`${nook}/src/sx/ratchet/index`)

const kem = loadSntrup761(readFileSync(`${nook}/src/sx/ratchet/sntrup761/sntrup761.wasm`))
const hex = (b: Uint8Array) => Buffer.from(b).toString("hex")
const te = new TextEncoder()
const V = currentE2EEncryptVersion

function make(pq: boolean) {
  const A = generateRcvE2EParams(V, pq, kem)
  const uri = paramsToUri(A.params, supportedE2EEncryptVRange)
  const B = generateSndE2EParams(V, replyKEM_(uri.kem, pq), kem)
  const bi = pqX3dhSnd(B.priv, uriToParams(uri, V))
  const dhrs = generateX448KeyPair()
  const vers = { current: V, maxSupported: 3 }
  const rcB = initSndRatchet(vers, uri.k2, dhrs, bi.init, bi.kemKeys)
  const bob = new RatchetSession(rcB, [], kem)
  const texts = ["hello alice", "second", "third", "fourth", "fifth", "sixth"]
  const msgs = texts.map((t) => hex(bob.encrypt(te.encode(t), 100, V)))
  const kp = (k: any) => (k ? { pk: hex(k.pk), sk: hex(k.sk) } : null)
  const privKem = (k: any) => (k ? (k.tag === "P" ? { tag: "P", kp: kp(k.kp) } : { tag: "A", ct: hex(k.ct), ss: hex(k.ss), kp: kp(k.kp) }) : null)
  return {
    pq,
    uriQuery: e2eParamsUriToString(uri),
    uriBin: hex(encodeE2ERatchetParamsUri(uri)),
    alice: { pk1: hex(A.priv.pk1), pk2: hex(A.priv.pk2), kem: privKem(A.priv.kem) },
    aliceParams: hex(encodeE2ERatchetParams(A.params)),
    bob: { pk1: hex(B.priv.pk1), pk2: hex(B.priv.pk2), kem: privKem(B.priv.kem), dhrs: hex(dhrs.priv) },
    bobParams: hex(encodeE2ERatchetParams(B.params)),
    init: { ad: hex(bi.init.assocData), vc: hex(bi.init.rcVerifyCodePQ), rk: hex(bi.init.ratchetKey), sndHK: hex(bi.init.sndHK), rcvNextHK: hex(bi.init.rcvNextHK) },
    texts,
    msgs,
  }
}
writeFileSync(process.argv[2], JSON.stringify({ off: make(false), on: make(true) }, null, 1))
