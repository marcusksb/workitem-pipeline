import { FormEvent, useState } from "react";
import { ErrorText } from "./parts";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export function Login({ onSubmit, error }: { onSubmit: (token: string) => void; error?: string }) {
  const [token, setToken] = useState("");
  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSubmit(token.trim());
  };
  return (
    <form className="mx-auto my-[20vh] grid max-w-[360px] gap-3 px-4" onSubmit={submit}>
      <h1 className="flex items-center gap-2 text-lg font-semibold"><img src="/favicon.svg" alt="" width={22} height={22} />Workload</h1>
      <p className="text-[13px] text-muted-foreground">Paste the token printed by workload, or open the URL it printed.</p>
      <Input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="token" autoFocus />
      <Button type="submit" variant="primary">Sign in</Button>
      {error && <ErrorText>{error}</ErrorText>}
    </form>
  );
}
