import {useId,useLayoutEffect,useRef,useState,type ReactNode} from 'react';
import {useTranslation} from 'react-i18next';
import {Bot,ChevronDown,ChevronUp,Copy,Folder,Link2,Package} from 'lucide-react';
import {Badge} from '@/components/ui/badge';
import {Skeleton} from '@/components/ui/skeleton';
import {OverflowTooltip} from '@/components/ui/overflow-tooltip';
import type {AgentInstallOption} from '@/bindings';
import type {WizardState} from '@/components/skills/add-skill/types';
import {RedirectHostConfirmation} from '@/components/source-discovery/RedirectHostConfirmation';
import './install-confirmation.css';

const PREVIEW_COUNT=3;

function Disclosure({expanded,onChange,controls,children,label}:{expanded:boolean;onChange:(expanded:boolean)=>void;controls:string;children:ReactNode;label?:string}){
  const Icon=expanded?ChevronUp:ChevronDown;
  return <button type="button" className="install-disclosure" aria-label={label} aria-expanded={expanded} aria-controls={controls} onClick={event=>{
    const trigger=event.currentTarget;onChange(!expanded);
    if(expanded)requestAnimationFrame(()=>trigger.scrollIntoView({block:'nearest'}));
  }}>{children}<Icon className="size-3.5" aria-hidden="true"/></button>;
}

export function InstallConfirmationSummary({state,isPreparing,blockedSkills,overwriteCount,defaultPath,directAgents,agentCount,linkOptions,copyOptions,onAcknowledge}:{
  state:WizardState;isPreparing:boolean;blockedSkills:Set<string>;overwriteCount:number;defaultPath:string;
  directAgents:string[];agentCount:number;linkOptions:AgentInstallOption[];copyOptions:AgentInstallOption[];onAcknowledge:(acknowledged:boolean)=>void;
}){
  const {t,i18n}=useTranslation();
  const [skillsExpanded,setSkillsExpanded]=useState(false);
  const [targetsExpanded,setTargetsExpanded]=useState(false);
  const [agentsExpanded,setAgentsExpanded]=useState(false);
  const [skillColumns,setSkillColumns]=useState(1);
  const skillsRef=useRef<HTMLUListElement>(null);
  useLayoutEffect(()=>{
    const element=skillsRef.current;if(!element)return;
    const update=(width:number)=>setSkillColumns(width>=560?2:1);
    update(element.clientWidth);
    const observer=new ResizeObserver(entries=>{const entry=entries[0];if(entry)update(entry.contentRect.width);});
    observer.observe(element);return()=>observer.disconnect();
  },[]);
  const id=useId();
  const candidates=new Map(state.availableSkills.map(skill=>[skill.name,skill]));
  const preparedByName=new Map(state.preparation.status==='ready'?state.preparation.prepared.preview.skills.map(skill=>[skill.skillName,skill]):[]);
  const attentionNames=new Set(state.selectedSkills.filter(name=>blockedSkills.has(name)||(state.overwrites[name]??[]).length>0||preparedByName.get(name)?.overridesLibrary||preparedByName.get(name)?.replacesDependency));
  const ordinaryPreview=new Set(state.selectedSkills.filter(name=>!attentionNames.has(name)).slice(0,PREVIEW_COUNT*skillColumns));
  const visibleSkills=state.selectedSkills.filter(name=>skillsExpanded||attentionNames.has(name)||ordinaryPreview.has(name));
  const canFoldSkills=state.selectedSkills.some(name=>!attentionNames.has(name)&&!ordinaryPreview.has(name));
  const targets=[{key:'shared',kind:'shared',name:t('addSkill.confirm.overview.plan.shared'),path:defaultPath},
    ...linkOptions.map(option=>({key:option.id,kind:'link',name:option.displayName,path:option.path})),
    ...copyOptions.map(option=>({key:option.id,kind:'copy',name:option.displayName,path:option.path})),
  ];
  const previewKeys=new Set(['shared']);
  for(const kind of ['link','copy']){const first=targets.find(target=>target.kind===kind);if(first)previewKeys.add(first.key);}
  for(const target of targets){if(previewKeys.size>=PREVIEW_COUNT)break;previewKeys.add(target.key);}
  const allTargetsVisible=targetsExpanded||attentionNames.size>0;
  const visibleTargets=targets.filter(target=>allTargetsVisible||previewKeys.has(target.key));
  const separator=i18n.language.startsWith('zh')?'、':', ';

  return <div className="install-confirmation" data-install-confirmation>
    <section className="install-summary-section">
      <div className="install-summary-heading" data-install-contents-section>
        <h2 data-skill-list-heading>{t('addSkill.confirm.itemsTitle')}</h2>
        <span data-install-skill-total>{t('addSkill.confirm.overview.skillCount',{count:state.selectedSkills.length})}</span>
      </div>
      {overwriteCount>0?<p className="install-summary-note">{t('addSkill.confirm.overwriteSkills',{count:overwriteCount})}</p>:null}
      <ul ref={skillsRef} id={`${id}-skills`} className="install-skill-list" data-columns={skillColumns} style={{gridTemplateColumns:`repeat(${skillColumns},minmax(0,1fr))`}}>
        {visibleSkills.map(name=>{
          const candidate=candidates.get(name),prepared=preparedByName.get(name);
          return <li key={name} data-install-skill={name} className={attentionNames.has(name)?'install-skill-attention':undefined}>
            <Package className="size-4 shrink-0 text-muted-foreground" aria-hidden="true"/>
            {isPreparing?<Skeleton className="h-4 w-32"/>:<OverflowTooltip content={name}><span className="install-item-name truncate" tabIndex={0} translate="no">{name}</span></OverflowTooltip>}
            {blockedSkills.has(name)?<Badge variant="destructive">{t('addSkill.confirm.directDownloadConflict')}</Badge>
              :prepared?.overridesLibrary?<Badge variant="outline">{t('addSkill.confirm.libraryOverrideBadge')}</Badge>
              :prepared?.replacesDependency?<Badge variant="outline">{t('addSkill.confirm.dependencyOverrideTitle')}</Badge>
              :(state.overwrites[name]??[]).length>0?<Badge variant="outline">{t('addSkill.confirm.overwriteGroup')}</Badge>:null}
            {candidate?.digestVerified?<Badge variant="outline">{t('addSkill.confirm.trust.digestVerified')}</Badge>:null}
          </li>;
        })}
      </ul>
      {canFoldSkills?<Disclosure expanded={skillsExpanded} onChange={setSkillsExpanded} controls={`${id}-skills`}>{t(skillsExpanded?'addSkill.confirm.overview.collapseSkills':'addSkill.confirm.overview.showAllSkills',{count:state.selectedSkills.length})}</Disclosure>:null}
    </section>
    <section className="install-summary-section">
      <div className="install-summary-heading">
        <h2>{t('addSkill.confirm.installPlan')}</h2>
        <span data-install-plan-total>{t('addSkill.confirm.overview.positionCount',{count:targets.length})}{agentCount>0?` · ${t('addSkill.confirm.overview.agentCount',{count:agentCount})}`:''}</span>
      </div>
      <ul id={`${id}-targets`} className="install-target-list" aria-label={t('addSkill.confirm.overview.positions')}>
        {visibleTargets.map(target=><li key={target.key} data-install-target={target.key}>
          <div className={`install-target-main${target.kind==='shared'?' install-target-main--shared':''}`}>
            <OverflowTooltip content={target.name}><span className="install-target-name" tabIndex={0}>{target.kind==='shared'?<Folder className="size-4 shrink-0 text-muted-foreground" aria-hidden="true"/>:<Bot className="size-4 shrink-0 text-muted-foreground" aria-hidden="true"/>}<span className="truncate">{target.name}</span></span></OverflowTooltip>
            <OverflowTooltip content={<code>{target.path}</code>}><code className="install-target-path truncate" tabIndex={0}>{target.path}</code></OverflowTooltip>
            {target.kind!=='shared'?<span className="install-target-mode">{target.kind==='link'?<Link2 className="size-3.5 shrink-0" aria-hidden="true"/>:<Copy className="size-3.5 shrink-0" aria-hidden="true"/>}{t(`addSkill.confirm.overview.plan.${target.kind}Short`)}</span>:<span className="install-common-agents">
              {directAgents.length===1?<OverflowTooltip content={directAgents[0]}><span className="truncate" tabIndex={0}>{directAgents[0]}</span></OverflowTooltip>
                :directAgents.length>1?<Disclosure expanded={agentsExpanded} onChange={setAgentsExpanded} controls={`${id}-agents`} label={t(agentsExpanded?'addSkill.confirm.overview.collapseAgents':'addSkill.confirm.overview.showAllAgents',{count:directAgents.length})}>{t('addSkill.confirm.overview.agentCount',{count:directAgents.length})}</Disclosure>:null}
            </span>}
          </div>
          {target.kind==='shared'&&directAgents.length>1?<div id={`${id}-agents`} className="install-common-agent-details" hidden={!agentsExpanded} data-install-shared-readers>{directAgents.join(separator)}</div>:null}
        </li>)}
      </ul>
      {targets.length>previewKeys.size&&attentionNames.size===0?<Disclosure expanded={targetsExpanded} onChange={setTargetsExpanded} controls={`${id}-targets`}>{t(targetsExpanded?'addSkill.confirm.overview.collapsePositions':'addSkill.confirm.overview.showAllPositions',{count:targets.length})}</Disclosure>:null}
    </section>
    {state.redirectedDownloadHost?<RedirectHostConfirmation sourceUrl={state.source} host={state.redirectedDownloadHost} acknowledged={state.redirectAcknowledged===true} onAcknowledgedChange={onAcknowledge}/>:null}
  </div>;
}
