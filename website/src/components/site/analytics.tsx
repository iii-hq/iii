import { ANALYTICS_ENABLED, CONSENT_KEY } from "@/lib/analytics"

// Consent-gated loaders for GTM, Common Room and PostHog, ported from the
// Astro site's AnalyticsHead. Rendered as plain inline <script>s in <head> so
// they run before hydration, exactly like the original. Each loader is a
// no-op until the visitor accepts the cookie banner (see writeConsent).
const GTM_ID = "GTM-N8DCTFB8"
const COMMON_ROOM_SITE = "da18833a-8f00-4ad0-9833-6608b59a713a"

const loaders = `(function(){
var K=${JSON.stringify(CONSENT_KEY)};
function ok(){try{return localStorage.getItem(K)==="accepted"}catch(e){return false}}
window.dataLayer=window.dataLayer||[];
window.iiiTrack=function(n,p){if(!ok())return;var o={event:n};if(p)for(var k in p)if(Object.prototype.hasOwnProperty.call(p,k))o[k]=p[k];window.dataLayer.push(o)};
window.iiiLoadGTM=function(){if(window.__iiiGTMLoaded||!ok())return;window.__iiiGTMLoaded=true;
window.dataLayer.push({"gtm.start":Date.now(),event:"gtm.js"});
var s=document.createElement("script");s.async=true;s.src="https://www.googletagmanager.com/gtm.js?id=${GTM_ID}";document.head.appendChild(s)};
window.iiiLoadCommonRoomSignals=function(){if(typeof window.signals!=="undefined"||!ok())return;
var s=document.createElement("script");s.async=true;s.src="https://cdn.cr-relay.com/v1/site/${COMMON_ROOM_SITE}/signals.js";
window.signals=Object.assign([],{_opts:{apiHost:"https://api.cr-relay.com"}},["page","identify","form"].reduce(function(a,m){a[m]=function(){window.signals.push([m,arguments]);return window.signals};return a},{}));
document.head.appendChild(s)};
window.iiiNotifyCommonRoomEmail=function(e){try{if(e&&ok()&&window.signals&&window.signals.form)window.signals.form({email:e})}catch(_){}};
window.iiiLoadGTM();window.iiiLoadCommonRoomSignals();
})();`

export function AnalyticsHead() {
  if (!ANALYTICS_ENABLED) return null
  return (
    <>
      {/* biome-ignore lint/security/noDangerouslySetInnerHtml: static first-party loader */}
      <script dangerouslySetInnerHTML={{ __html: loaders }} />
      <script src="/posthog-consent.js" async />
    </>
  )
}

export function AnalyticsNoScript() {
  if (!ANALYTICS_ENABLED) return null
  return (
    <noscript>
      <iframe
        title="Google Tag Manager"
        src={`https://www.googletagmanager.com/ns.html?id=${GTM_ID}`}
        sandbox=""
        height="0"
        width="0"
        style={{ display: "none", visibility: "hidden" }}
      />
    </noscript>
  )
}
