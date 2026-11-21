"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  burnRate,
  ensureConnected,
  formatAmount,
  loadEmployeeStreams,
  loadEmployerStreams,
  shortAddr,
  STREAM_CONTRACT_ID,
  withdraw,
  withdrawAll,
  type StreamRow,
} from "@/lib/stellar";
import LinearVestProgress from "./LinearVestProgress";

type Tab = "employer" | "employee";

export default function Dashboard() {
  const [address, setAddress] = useState("");
  const [tab, setTab] = useState<Tab>("employer");
  const [employerStreams, setEmployerStreams] = useState<StreamRow[]>([]);
  const [employeeStreams, setEmployeeStreams] = useState<StreamRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState("");

  const refresh = useCallback(async (addr: string) => {
    setLoading(true);
    setError("");
    try {
      const [emp, epi] = await Promise.all([
        loadEmployerStreams(addr),
        loadEmployeeStreams(addr),
      ]);
      setEmployerStreams(emp);
      setEmployeeStreams(epi);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setLoading(false);
    }
  }, []);

  const connect = async () => {
    try {
      const addr = await ensureConnected();
      setAddress(addr);
      await refresh(addr);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  };

  useEffect(() => {
    if (!STREAM_CONTRACT_ID) {
      setError("NEXT_PUBLIC_STREAM_CONTRACT_ID is not set — copy .env.example to .env.local.");
    }
  }, []);

  const doWithdraw = async (streamId: bigint) => {
    setBusyId(streamId.toString());
    setError("");
    try {
      await withdraw(address, streamId);
      await refresh(address);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusyId(null);
    }
  };

  const doWithdrawAll = async () => {
    setBusyId("all");
    setError("");
    try {
      await withdrawAll(address);
      await refresh(address);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusyId(null);
    }
  };

  const streams = tab === "employer" ? employerStreams : employeeStreams;
  const totalClaimable = streams
    .filter((s) => s.status_label === "Active")
    .reduce((sum, s) => sum + s.claimable, 0n);

  return (
    <main>
      <header>
        <div>
          <h1>PayStream Dashboard</h1>
          <div className="muted">
            contract <code>{STREAM_CONTRACT_ID ? shortAddr(STREAM_CONTRACT_ID) : "unset"}</code> · Stellar testnet
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          {address && (
            <button className="secondary" onClick={() => refresh(address)} disabled={loading}>
              {loading ? "Refreshing…" : "Refresh"}
            </button>
          )}
          <button onClick={connect} disabled={loading}>
            {address ? shortAddr(address) : "Connect Freighter"}
          </button>
        </div>
      </header>

      {error && <div className="error">{error}</div>}

      {!address ? (
        <div className="notice">
          Connect your Freighter wallet to view streams. The dashboard reads streams where the
          connected account is employer or employee on the configured testnet contract.
        </div>
      ) : (
        <>
          <div className="tabs">
            <button className={tab === "employer" ? "active" : ""} onClick={() => setTab("employer")}>
              Employer ({employerStreams.length})
            </button>
            <button className={tab === "employee" ? "active" : ""} onClick={() => setTab("employee")}>
              Employee ({employeeStreams.length})
            </button>
          </div>

          {tab === "employer" ? (
            <div className="cards">
              <Stat label="Streams" value={String(employerStreams.length)} />
              <Stat label="Burn rate" value={`${formatAmount(burnRate(employerStreams))}/s`} />
              <Stat label="Total claimable" value={formatAmount(totalClaimable)} />
              <Stat
                label="Total remaining"
                value={formatAmount(employerStreams.reduce((s, x) => s + x.remaining, 0n))}
              />
            </div>
          ) : (
            <div className="cards">
              <Stat label="Streams" value={String(employeeStreams.length)} />
              <Stat label="Claimable now" value={formatAmount(totalClaimable)} />
            </div>
          )}

          {tab === "employee" && totalClaimable > 0n && (
            <p>
              <button onClick={doWithdrawAll} disabled={busyId !== null}>
                {busyId === "all" ? "Withdrawing…" : "Withdraw all"}
              </button>
            </p>
          )}

          {streams.length === 0 ? (
            <div className="notice">No {tab} streams for this account.</div>
          ) : (
            <StreamTable
              tab={tab}
              streams={streams}
              busyId={busyId}
              onWithdraw={doWithdraw}
            />
          )}
        </>
      )}
    </main>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="card">
      <div className="label">{label}</div>
      <div className="value">{value}</div>
    </div>
  );
}

function StreamTable({
  tab,
  streams,
  busyId,
  onWithdraw,
}: {
  tab: Tab;
  streams: StreamRow[];
  busyId: string | null;
  onWithdraw: (id: bigint) => void;
}) {
  return (
    <table>
      <thead>
        <tr>
          <th>ID</th>
          <th>{tab === "employer" ? "Employee" : "Employer"}</th>
          <th>Status</th>
          <th>Claimable</th>
          <th>Burn rate</th>
          <th>Progress</th>
          {tab === "employee" && <th />}
        </tr>
      </thead>
      <tbody>
        {streams.map((s) => (
          <tr key={s.id.toString()}>
            <td>{s.id.toString()}</td>
            <td>
              <code>{shortAddr(tab === "employer" ? s.employee : s.employer)}</code>
            </td>
            <td>
              <span className={`pill ${s.status_label}`}>{s.status_label}</span>
            </td>
            <td>{formatAmount(s.claimable)}</td>
            <td>{s.status_label === "Active" ? `${formatAmount(s.rate_per_second)}/s` : "—"}</td>
            <td>
              {s.stream_type === "LinearVest" ? (
                <LinearVestProgress stream={s} />
              ) : (
                <span className="muted">—</span>
              )}
            </td>
            {tab === "employee" && (
              <td style={{ textAlign: "right" }}>
                <button
                  onClick={() => onWithdraw(s.id)}
                  disabled={s.status_label !== "Active" || s.claimable <= 0n || busyId !== null}
                >
                  {busyId === s.id.toString() ? "…" : "Withdraw"}
                </button>
              </td>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
