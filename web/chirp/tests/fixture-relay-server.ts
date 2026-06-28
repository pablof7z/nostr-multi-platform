import type { AddressInfo } from "node:net";
import { createServer as createHttpsServer, type Server as HttpsServer } from "node:https";
import { WebSocketServer } from "ws";

const LOCALHOST_CERT = `-----BEGIN CERTIFICATE-----
MIICyTCCAbGgAwIBAgIJAO71ZUmFq5CVMA0GCSqGSIb3DQEBCwUAMBQxEjAQBgNV
BAMMCWxvY2FsaG9zdDAeFw0yNjA2MjcwODMwNDNaFw0zNjA2MjQwODMwNDNaMBQx
EjAQBgNVBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoC
ggEBANV5naHLIfu9777SdPJJcOXyyRwK69nx+OmO7Z6Vsq2mmb1+BLXXgq1GXKWN
AO0eCAj2owX/YsTrkI78jKKpYeh0d2619AfDH92KYbKVzZM03XeUXQVQ3OCac0Zf
XmNTsHyaGaICOs9oOinICX9iZtqooBDzq8vWBzaM295jVQFuLXBoWoakJwzQm/jZ
i2M7rTYLTKyop1fGsVycfz13AW7KtLhtM2fvhpP58PQxfv+p0sM5K2P/s0bSMDzx
ZsX/xfU7WYjtr6jWuH2epklE6bNh9WnRAkuQespDfmfKMyNUAPtct1acHiK939jh
kQ/oIcIRwxG2YbW8aSmwCagMXBsCAwEAAaMeMBwwGgYDVR0RBBMwEYIJbG9jYWxo
b3N0hwR/AAABMA0GCSqGSIb3DQEBCwUAA4IBAQC7VniE67inAv7gpc5XGnpstF6+
3Bq/ryzgU5ZvN7cTXE9vBZl0N2XRzAONUJP5b7BFuECEsjzGWfUDK9gwAB+Z0rYI
hGZGtuBHz8wRzeA0b7YwMOSnpdWcgkskuyFJAeFQnu0MijfHXfC+iqkhSkbiIvi3
3F+3Jpogak63kDNsGMhWRahQg39NnP/VX6QRwtUZWJ9f+DtLfLw+M6laT9lMfLy9
S3I/E8mSEK18T0wpATyq1z/GSlqx2f9tw9r42ZKvjMcV+a/8dWt+BzhB8Y1IdQic
77nd6VPYgu2YwuAGeZ9FTfuNmRJ5sXIVKG9tHh5iRSErjIF+TNi+4tBPeig0
-----END CERTIFICATE-----`;

const LOCALHOST_KEY = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDVeZ2hyyH7ve++
0nTySXDl8skcCuvZ8fjpju2elbKtppm9fgS114KtRlyljQDtHggI9qMF/2LE65CO
/IyiqWHodHdutfQHwx/dimGylc2TNN13lF0FUNzgmnNGX15jU7B8mhmiAjrPaDop
yAl/YmbaqKAQ86vL1gc2jNveY1UBbi1waFqGpCcM0Jv42YtjO602C0ysqKdXxrFc
nH89dwFuyrS4bTNn74aT+fD0MX7/qdLDOStj/7NG0jA88WbF/8X1O1mI7a+o1rh9
nqZJROmzYfVp0QJLkHrKQ35nyjMjVAD7XLdWnB4ivd/Y4ZEP6CHCEcMRtmG1vGkp
sAmoDFwbAgMBAAECggEANNwccEfAbnmlt/adBrGwxv/LVKpPpHQKUqsVo8Mlr2Il
h4qA2BY5QXa+0i+MHyrkinOoAoAukNxTu1RF7rFKkSjlugBMIO/sIAt4DaSSdvUM
MeQG9J4FU4hrKu3KjYXXmcL+veMXdOw2Gspxr51KIrLGj+wGij7BInzWpMar8eJG
cr62asqQosfhAl8TC3rMGmILV+0PZQOqx9U+rD1/wLMLPaq6aVKgdHnyVK9BMab2
b1NvrTsEPqcWVsmwWuI/WANSmUH9mToP40/pPIkgGEVj7HmDf5p+wO0l4H0Fzmlf
DHqxJbAjA/BzNtsEjJqhxa+DuB5iP6eoIjTUE442IQKBgQD0bA3Bm72+ffQTgsd8
Nw/4F0+m2uunHSYxB3qo0YjWx8Z7sqThKRNiCmqA7Kg8UY9Usuz1ejgW21Y0Ccfd
t5dP0EzCSyX3ND9kjPPYg8tJJ6GWA3ZKHXA15VanoNyuFzYtPDA5KdLE4vvgea4S
4OCM/SLz1ckNya43/BwC3LWBzQKBgQDflkmrAPhGZjdmywC/T0nwerM6Y//mAms/
WLqAhJmck+p6gpCaHdeCIUel0BvQY+T7SF1jxufVDtzkccIAHlTdd73FzPJU5QrI
6EcosU8GLeWpg4wguHmFA4fVXO+Q2ynLEqDziVXMyRLX0ZFVTOOjp/MD3a8zA7As
25hYMDONhwKBgHjgs1Drj8QUkE/R3owMwyYDiU3QeLh1zvxyYXP55D2sIPnt2GLO
KJrU4eUOpQjnoQXXUx73qaOMJ66mo6R/9iHvtvZjqcv+l9dHahTK4Q81vVDuN2Lh
+it9VwShpmGmcxGd8Y9joqviQYS7SJ5nfkXbrpx+PudFtZZUZRn5Qv6FAoGBAMKo
8+Zf58hTAfUK1NG61GL8UMKLgaXdgUYbl/SAfcTmuwSCXCbxyElRdDGWqECcWCW2
cSiHahwC3qo9qGu1/Kj8sUpfDrR+3Q7hu+JfzK9SklstXni2Y4Y89qv6R9DUHuTg
iSS+8uZiQXeIy4F6ec5oUJmTDg/aLC5B2bcd8CRNAoGBAJZEYTZu6rynP9EqvCUW
oc/D+o/BWEXWzSmKtEXnvk85DNBwZ3IcocDFdAYQAAY2yfT7HDM2/tL3NVHI72tf
YMcvwh8EKMaiioclZv9R7Tse0NG04JwJqG5Pgw9V7qvZuWPfyQIFoCs0gCZVSC6b
1YvaSO/sxHd9aIvoSTtFizi1
-----END PRIVATE KEY-----`;

export type StartedFixtureRelayServer = {
  wss: WebSocketServer;
  url: string;
  close(): Promise<void>;
};

export function startFixtureWebSocketServer(secure: boolean): Promise<StartedFixtureRelayServer> {
  return secure ? startSecureFixtureWebSocketServer() : startPlainFixtureWebSocketServer();
}

function startPlainFixtureWebSocketServer(): Promise<StartedFixtureRelayServer> {
  return new Promise((resolve, reject) => {
    const wss = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    wss.once("error", reject);
    wss.once("listening", () => {
      const { port } = wss.address() as AddressInfo;
      resolve({ wss, url: `ws://127.0.0.1:${port}`, close: () => closeWebSocketServer(wss) });
    });
  });
}

function startSecureFixtureWebSocketServer(): Promise<StartedFixtureRelayServer> {
  return new Promise((resolve, reject) => {
    const server = createHttpsServer({ key: LOCALHOST_KEY, cert: LOCALHOST_CERT });
    const wss = new WebSocketServer({ server });
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as AddressInfo;
      resolve({
        wss,
        url: `wss://127.0.0.1:${port}`,
        close: () => closeWebSocketServer(wss, server),
      });
    });
  });
}

function closeWebSocketServer(wss: WebSocketServer, server?: HttpsServer): Promise<void> {
  for (const client of wss.clients) client.terminate();
  return new Promise<void>((resolve, reject) => {
    wss.close((wsErr) => {
      if (wsErr) {
        reject(wsErr);
        return;
      }
      if (server === undefined) {
        resolve();
        return;
      }
      server.close((serverErr) => (serverErr ? reject(serverErr) : resolve()));
    });
  });
}
