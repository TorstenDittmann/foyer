// Passkey sign-in and registration for the Foyer website.
//
// Buttons opt in with data attributes, so this works on pages rendered by the
// server and on pages the router renders later:
//
//   <button type="button" data-passkey="login" data-next="/account"
//           data-status="some-id">Sign in with a passkey</button>
//   <button type="button" data-passkey="register" data-name-input="some-id"
//           data-status="some-id">Add a passkey</button>
//
// The website relays each ceremony to the API (see `routes.rs`), so the
// session token never reaches this script.
(function () {
  "use strict";

  function toBytes(b64url) {
    var b64 = b64url.replace(/-/g, "+").replace(/_/g, "/");
    while (b64.length % 4) b64 += "=";
    var raw = atob(b64);
    var bytes = new Uint8Array(raw.length);
    for (var i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
    return bytes.buffer;
  }

  function toB64url(buffer) {
    if (!buffer) return null;
    var bytes = new Uint8Array(buffer);
    var raw = "";
    for (var i = 0; i < bytes.length; i++) raw += String.fromCharCode(bytes[i]);
    return btoa(raw).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  function descriptors(list) {
    return (list || []).map(function (c) {
      return Object.assign({}, c, { id: toBytes(c.id) });
    });
  }

  async function post(path, body) {
    var response = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify(body || {}),
    });
    var data = null;
    try {
      data = await response.json();
    } catch (_) {
      // Fall through to the generic message.
    }
    if (!response.ok) {
      throw new Error((data && data.message) || "Something went wrong. Try again.");
    }
    return data;
  }

  function say(button, text, tone) {
    var el = button.dataset.status && document.getElementById(button.dataset.status);
    if (!el) return;
    el.textContent = text || "";
    el.dataset.tone = tone || "";
    el.hidden = !text;
  }

  // The browser cancelled or the user closed the prompt: not worth an error.
  function cancelled(err) {
    return err && (err.name === "NotAllowedError" || err.name === "AbortError");
  }

  async function login(button) {
    var challenge = await post("/auth/passkey/login/start");
    var options = challenge.options.publicKey;
    options.challenge = toBytes(options.challenge);
    options.allowCredentials = descriptors(options.allowCredentials);
    var credential = await navigator.credentials.get({ publicKey: options });
    var result = await post("/auth/passkey/login/finish", {
      ceremony: challenge.ceremony,
      next: button.dataset.next || null,
      credential: {
        id: credential.id,
        rawId: toB64url(credential.rawId),
        type: credential.type,
        response: {
          authenticatorData: toB64url(credential.response.authenticatorData),
          clientDataJSON: toB64url(credential.response.clientDataJSON),
          signature: toB64url(credential.response.signature),
          userHandle: toB64url(credential.response.userHandle),
        },
        extensions: credential.getClientExtensionResults(),
      },
    });
    window.location.assign(result.redirect || "/account");
  }

  async function register(button) {
    var input = button.dataset.nameInput && document.getElementById(button.dataset.nameInput);
    var challenge = await post("/auth/passkey/register/start");
    var options = challenge.options.publicKey;
    options.challenge = toBytes(options.challenge);
    options.user.id = toBytes(options.user.id);
    options.excludeCredentials = descriptors(options.excludeCredentials);
    var credential = await navigator.credentials.create({ publicKey: options });
    var response = credential.response;
    await post("/auth/passkey/register/finish", {
      ceremony: challenge.ceremony,
      name: input ? input.value : "",
      credential: {
        id: credential.id,
        rawId: toB64url(credential.rawId),
        type: credential.type,
        response: {
          attestationObject: toB64url(response.attestationObject),
          clientDataJSON: toB64url(response.clientDataJSON),
          transports: response.getTransports ? response.getTransports() : undefined,
        },
        extensions: credential.getClientExtensionResults(),
      },
    });
    say(button, "Passkey added.", "good");
    window.location.reload();
  }

  document.addEventListener("click", async function (event) {
    var button = event.target.closest && event.target.closest("[data-passkey]");
    if (!button || button.disabled) return;
    event.preventDefault();
    if (!window.PublicKeyCredential || !navigator.credentials) {
      say(button, "This browser doesn't support passkeys.", "bad");
      return;
    }
    button.disabled = true;
    say(button, "Waiting for your passkey…", "");
    try {
      if (button.dataset.passkey === "login") {
        await login(button);
      } else if (button.dataset.passkey === "register") {
        await register(button);
      }
    } catch (err) {
      if (cancelled(err)) {
        say(button, "", "");
      } else if (err && err.name === "InvalidStateError") {
        say(button, "This device already has a passkey for your account.", "bad");
      } else {
        say(button, (err && err.message) || "That didn't work. Try again.", "bad");
      }
    } finally {
      button.disabled = false;
    }
  });
})();
