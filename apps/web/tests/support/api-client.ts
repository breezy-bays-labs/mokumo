/**
 * Standalone API client for Mokumo's backend.
 * Uses plain fetch() — ZERO Playwright dependencies.
 * Shared by E2E fixtures and the seed script.
 */

export interface SetupCredentials {
  setupToken: string;
  adminEmail: string;
  adminName: string;
  adminPassword: string;
  shopName: string;
}

/**
 * Run the setup wizard on a fresh Axum backend.
 * Throws on non-2xx response.
 */
export async function runSetupWizard(baseUrl: string, creds: SetupCredentials): Promise<void> {
  const res = await fetch(`${baseUrl}/api/setup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      setup_token: creds.setupToken,
      admin_email: creds.adminEmail,
      admin_name: creds.adminName,
      admin_password: creds.adminPassword,
      shop_name: creds.shopName,
    }),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`Setup wizard failed (${res.status}): ${body}`);
  }
}

export interface LoginResult {
  /** The raw Set-Cookie header value from the login response. */
  setCookie: string;
}

/**
 * Login via API and return the session cookie.
 * Throws on non-2xx response or missing Set-Cookie header.
 */
export async function login(
  baseUrl: string,
  email: string,
  password: string,
): Promise<LoginResult> {
  const res = await fetch(`${baseUrl}/api/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`Login failed (${res.status}): ${body}`);
  }

  const setCookie = res.headers.get("set-cookie");
  if (!setCookie) {
    throw new Error(
      "Login succeeded but no Set-Cookie header was returned. " +
        "Check the backend's Set-Cookie header and SameSite attributes.",
    );
  }

  return { setCookie };
}
