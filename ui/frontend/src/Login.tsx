import { FormEvent, useState } from "react";

export function Login({ onSubmit, error }: { onSubmit: (token: string) => void; error?: string }) {
  const [token, setToken] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSubmit(token.trim());
  };
  return (
    <form className="login" onSubmit={submit}>
      <h1 className="brand"><img src="/favicon.svg" alt="" width={22} height={22} />Workload</h1>
      <p>Paste the token printed by workload, or open the URL it printed.</p>
      <input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="token" autoFocus />
      <button type="submit" className="primary">Sign in</button>
      {error && <p className="error">{error}</p>}
    </form>
  );
}
