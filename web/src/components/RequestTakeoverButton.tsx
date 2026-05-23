import { useState } from 'react';
import { requestProjectTakeover } from '../api/client';
import { Button } from './Button';

export function RequestTakeoverButton() {
  const [isSending, setIsSending] = useState(false);
  const [sent, setSent] = useState(false);

  const handleRequest = async () => {
    setIsSending(true);
    try {
      await requestProjectTakeover();
      setSent(true);
      setTimeout(() => setSent(false), 5000);
    } catch {
      // Silently ignore; the owner may already have changed
    } finally {
      setIsSending(false);
    }
  };

  if (sent) {
    return (
      <span className="rounded-md bg-amber-500/15 px-2 py-0.5 text-xs text-amber-400">
        Request sent
      </span>
    );
  }

  return (
    <Button
      className="h-6 px-2 py-0 text-xs border-amber-500/30 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20"
      disabled={isSending}
      onClick={() => void handleRequest()}
      type="button"
    >
      {isSending ? 'Requesting…' : 'Request Takeover'}
    </Button>
  );
}
