"use client";

import { useCallback, useEffect, useState } from "react";
import {
  isConnected as freighterIsConnected,
  requestAccess,
  getAddress,
  signTransaction,
} from "@stellar/freighter-api";
import { STELLAR } from "./config";

export interface StellarWallet {
  address: string | null;
  isConnected: boolean;
  connecting: boolean;
  error: string | null;
  connect: () => Promise<void>;
  /** Sign a transaction XDR with Freighter; returns the signed XDR. */
  sign: (xdr: string) => Promise<string>;
}

/** Minimal Freighter-backed wallet. No global provider needed. */
export function useStellarWallet(): StellarWallet {
  const [address, setAddress] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // On mount, silently pick up an already-authorized address.
  useEffect(() => {
    (async () => {
      try {
        const c = await freighterIsConnected();
        if (!c.isConnected) return;
        const a = await getAddress();
        if (a.address) setAddress(a.address);
      } catch {
        /* extension not installed / locked — stay disconnected */
      }
    })();
  }, []);

  const connect = useCallback(async () => {
    setConnecting(true);
    setError(null);
    try {
      const res = await requestAccess();
      if (res.error) throw new Error(res.error);
      setAddress(res.address);
    } catch (e) {
      setError(
        e instanceof Error
          ? e.message
          : "Freighter not found — install the extension"
      );
    } finally {
      setConnecting(false);
    }
  }, []);

  const sign = useCallback(
    async (xdr: string) => {
      if (!address) throw new Error("Wallet not connected");
      const res = await signTransaction(xdr, {
        address,
        networkPassphrase: STELLAR.networkPassphrase,
      });
      if (res.error) throw new Error(res.error);
      return res.signedTxXdr;
    },
    [address]
  );

  return {
    address,
    isConnected: !!address,
    connecting,
    error,
    connect,
    sign,
  };
}
