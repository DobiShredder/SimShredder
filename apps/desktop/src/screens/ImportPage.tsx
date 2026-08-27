import { FileInput, RefreshCw, RotateCcw, Star, Upload, UserRound } from "lucide-react";
import { useEffect, useRef, useState, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  characterProfiles,
  reloadCharacterProfileFromArmory,
  restorePreviousCharacterProfileInput,
  saveCharacterProfileImport,
  setCharacterProfileFavorite,
  type CharacterProfile,
} from "../profiles";
import {
  defaultQuickRequest,
  quickPrepare,
  type PreparedQuickSim,
  type QuickSimRequest,
  type SourceFormat,
} from "../quick";

export function ImportPage({ onPrepared }: {
  onPrepared: (request: QuickSimRequest, preview: PreparedQuickSim) => void;
}) {
  const { t } = useTranslation();
  const [source, setSource] = useState("");
  const [format, setFormat] = useState<SourceFormat>("addonExport");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [profiles, setProfiles] = useState<CharacterProfile[]>([]);
  const [profilesError, setProfilesError] = useState<string | null>(null);
  const [selectedArmoryProfile, setSelectedArmoryProfile] = useState<CharacterProfile | null>(null);
  const [profileBusy, setProfileBusy] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const sortProfiles = (values: CharacterProfile[]) => [...values].sort((left, right) =>
    Number(right.favorite) - Number(left.favorite)
      || right.capturedAtUnixSeconds - left.capturedAtUnixSeconds
      || left.displayName.localeCompare(right.displayName),
  );
  const replaceProfile = (next: CharacterProfile) => {
    setProfiles((current) => sortProfiles(current.map((profile) => profile.id === next.id ? next : profile)));
  };

  useEffect(() => {
    void characterProfiles()
      .then((stored) => setProfiles(sortProfiles(stored)))
      .catch((reason) => setProfilesError(String(reason)));
  }, []);

  const loadFile = async (file: File) => {
    if (file.size > 2 * 1024 * 1024) {
      setError(t("importPage.fileTooLarge"));
      return;
    }
    setSource(await file.text());
    setFormat("simcFile");
    setError(null);
  };
  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const file = event.dataTransfer.files.item(0);
    if (file) void loadFile(file);
  };
  const review = async () => {
    setBusy(true);
    setError(null);
    const request = defaultQuickRequest(source, format);
    try {
      const preview = await quickPrepare(request);
      const saved = await saveCharacterProfileImport(request);
      setProfiles((current) => sortProfiles([...current.filter((profile) => profile.id !== saved.id), saved]));
      onPrepared(request, preview);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const useProfile = async (profile: CharacterProfile) => {
    setProfileBusy(profile.id);
    setProfilesError(null);
    try {
      onPrepared(profile.request, await quickPrepare(profile.request));
    } catch (reason) {
      setProfilesError(String(reason));
    } finally {
      setProfileBusy(null);
    }
  };
  const toggleFavorite = async (profile: CharacterProfile) => {
    setProfileBusy(profile.id);
    setProfilesError(null);
    try {
      replaceProfile(await setCharacterProfileFavorite(profile.id, !profile.favorite));
    } catch (reason) {
      setProfilesError(String(reason));
    } finally {
      setProfileBusy(null);
    }
  };
  const reloadFromArmory = async () => {
    if (!selectedArmoryProfile?.armoryRefresh.available) return;
    setProfileBusy(selectedArmoryProfile.id);
    setProfilesError(null);
    try {
      const refreshed = await reloadCharacterProfileFromArmory(selectedArmoryProfile.id);
      replaceProfile(refreshed);
      setSelectedArmoryProfile(null);
      onPrepared(refreshed.request, await quickPrepare(refreshed.request));
    } catch (reason) {
      setProfilesError(String(reason));
    } finally {
      setProfileBusy(null);
    }
  };
  const restorePrevious = async (profile: CharacterProfile) => {
    setProfileBusy(profile.id);
    setProfilesError(null);
    try {
      const restored = await restorePreviousCharacterProfileInput(profile.id);
      replaceProfile(restored);
      onPrepared(restored.request, await quickPrepare(restored.request));
    } catch (reason) {
      setProfilesError(String(reason));
    } finally {
      setProfileBusy(null);
    }
  };

  return (
    <div className="page import-page">
      <p className="eyebrow">{t("importPage.eyebrow")}</p>
      <h1>{t("importPage.title")}</h1>
      <p className="settings-lead">{t("importPage.body")}</p>
      <section className="saved-profiles" aria-labelledby="saved-profiles-title">
        <div className="section-heading">
          <div>
            <h2 id="saved-profiles-title">{t("profiles.title")}</h2>
            <p>{t("profiles.body")}</p>
          </div>
        </div>
        {profiles.length ? (
          <div className="profile-card-grid">
            {profiles.map((profile) => (
              <article className="profile-card" key={profile.id}>
                <div className="profile-card-heading">
                  <div className="card-icon"><UserRound aria-hidden="true" size={19} /></div>
                  <div>
                    <h3>{profile.displayName}</h3>
                    <p>{[profile.identity.region, profile.identity.realm].filter(Boolean).join(" · ") || t("profiles.identityUnknown")}</p>
                  </div>
                  <button
                    aria-label={t(profile.favorite ? "profiles.unfavorite" : "profiles.favorite", { name: profile.displayName })}
                    aria-pressed={profile.favorite}
                    className="icon-button profile-favorite"
                    disabled={profileBusy === profile.id}
                    onClick={() => void toggleFavorite(profile)}
                    type="button"
                  ><Star aria-hidden="true" fill={profile.favorite ? "currentColor" : "none"} size={18} /></button>
                </div>
                <p className="profile-meta">{profile.class} · {profile.specialization}</p>
                <p className="profile-meta">{t(`profiles.sources.${profile.inputSource}`)} · {new Date(profile.capturedAtUnixSeconds * 1000).toLocaleString()}</p>
                <div className="profile-actions">
                  <button className="primary-button" disabled={profileBusy === profile.id} onClick={() => void useProfile(profile)} type="button">
                    <FileInput aria-hidden="true" size={17} />{t("profiles.use")}
                  </button>
                  <button
                    className="secondary-button"
                    disabled={profileBusy === profile.id || !profile.armoryRefresh.available}
                    onClick={() => setSelectedArmoryProfile(profile)}
                    title={!profile.armoryRefresh.available ? t("profiles.armoryUnavailable") : undefined}
                    type="button"
                  >
                    <RefreshCw aria-hidden="true" size={17} />{t("profiles.reloadArmory")}
                  </button>
                  {profile.previousInputAvailable ? (
                    <button className="text-button" disabled={profileBusy === profile.id} onClick={() => void restorePrevious(profile)} type="button">
                      <RotateCcw aria-hidden="true" size={16} />{t("profiles.restorePrevious")}
                    </button>
                  ) : null}
                </div>
              </article>
            ))}
          </div>
        ) : <p className="profile-empty">{t("profiles.empty")}</p>}
        {profilesError ? <div className="inline-error" role="alert"><strong>{t("profiles.errorTitle")}</strong><code>{profilesError}</code></div> : null}
      </section>
      <div className="section-heading import-heading"><h2>{t("profiles.importTitle")}</h2></div>
      <fieldset className="format-choice">
        <legend>{t("importPage.sourceLabel")}</legend>
        {(["addonExport", "simcFile"] as const).map((value) => (
          <label key={value}>
            <input checked={format === value} name="source-format" onChange={() => setFormat(value)} type="radio" />
            <span>{t(value === "addonExport" ? "importPage.addon" : "importPage.simc")}</span>
          </label>
        ))}
      </fieldset>
      <div className="source-drop" onDragOver={(event) => event.preventDefault()} onDrop={handleDrop}>
        <textarea aria-label={t("importPage.sourceLabel")} onChange={(event) => setSource(event.target.value)} placeholder={t("importPage.sourcePlaceholder")} spellCheck={false} value={source} />
        <div className="source-actions">
          <span><Upload aria-hidden="true" size={16} />{t("importPage.dropHint")}</span>
          <button className="secondary-button" type="button" onClick={() => fileInput.current?.click()}><FileInput aria-hidden="true" size={17} />{t("importPage.chooseFile")}</button>
          <input ref={fileInput} accept=".simc,text/plain" aria-label={t("importPage.chooseFile")} className="sr-only" onChange={(event) => { const file = event.target.files?.item(0); if (file) void loadFile(file); }} type="file" />
        </div>
      </div>
      {error ? <div className="inline-error" role="alert"><strong>{t("importPage.errorTitle")}</strong><code>{error}</code></div> : null}
      <div className="button-row">
        <button className="primary-button" disabled={busy || !source.trim()} type="button" onClick={() => void review()}><FileInput aria-hidden="true" size={18} />{busy ? t("importPage.reviewing") : t("importPage.review")}</button>
      </div>
      {selectedArmoryProfile ? (
        <div className="modal-backdrop">
          <dialog className="update-dialog" open aria-labelledby="armory-reload-title" aria-describedby="armory-reload-description">
            <p className="eyebrow">{t("profiles.armoryEyebrow")}</p>
            <h2 id="armory-reload-title">{t("profiles.armoryTitle", { name: selectedArmoryProfile.displayName })}</h2>
            <p id="armory-reload-description">{t("profiles.armoryBody")}</p>
            {!selectedArmoryProfile.armoryRefresh.available ? <p className="inline-notice">{t("profiles.armoryUnavailable")}</p> : null}
            <div className="button-row">
              <button className="primary-button" disabled={!selectedArmoryProfile.armoryRefresh.available || profileBusy === selectedArmoryProfile.id} onClick={() => void reloadFromArmory()} type="button">
                <RefreshCw aria-hidden="true" size={17} />{t("profiles.armoryConfirm")}
              </button>
              <button className="secondary-button" autoFocus onClick={() => setSelectedArmoryProfile(null)} type="button">{t("profiles.cancel")}</button>
            </div>
          </dialog>
        </div>
      ) : null}
    </div>
  );
}
