/**
 * Channel Type Definitions
 * Types for messaging channels (WhatsApp, Telegram, etc.)
 */

/**
 * Supported channel types
 */
export type ChannelType =
  | 'whatsapp'
  | 'dingtalk'
  | 'qqbot'
  | 'wecom'
  | 'wecom-app'
  | 'telegram'
  | 'discord'
  | 'signal'
  | 'feishu'
  | 'imessage'
  | 'matrix'
  | 'line'
  | 'msteams'
  | 'googlechat'
  | 'mattermost';

/**
 * Channel connection status
 */
export type ChannelStatus = 'connected' | 'disconnected' | 'connecting' | 'error';

/**
 * Channel connection type
 */
export type ChannelConnectionType = 'token' | 'qr' | 'oauth' | 'webhook';

/**
 * Channel data structure
 */
export interface Channel {
  id: string;
  type: ChannelType;
  name: string;
  status: ChannelStatus;
  accountId?: string;
  lastActivity?: string;
  error?: string;
  avatar?: string;
  metadata?: Record<string, unknown>;
}

/**
 * Channel configuration field definition
 */
export interface ChannelConfigField {
  key: string;
  label: string;
  type: 'text' | 'password' | 'select' | 'textarea';
  placeholder?: string;
  required?: boolean;
  envVar?: string;
  description?: string;
  options?: { value: string; label: string }[];
  defaultValue?: string;
}

/**
 * Channel metadata with configuration info
 */
export interface ChannelMeta {
  id: ChannelType;
  name: string;
  icon: string;
  description: string;
  connectionType: ChannelConnectionType;
  docsUrl: string;
  configFields: ChannelConfigField[];
  instructions: string[];
  isPlugin?: boolean;
}

/**
 * Channel icons mapping
 */
export const CHANNEL_ICONS: Record<ChannelType, string> = {
  whatsapp: '📱',
  dingtalk: '💬',
  qqbot: '🐧',
  wecom: '🏢',
  'wecom-app': '🧩',
  telegram: '✈️',
  discord: '🎮',
  signal: '🔒',
  feishu: '🐦',
  imessage: '💬',
  matrix: '🔗',
  line: '🟢',
  msteams: '👔',
  googlechat: '💭',
  mattermost: '💠',
};

/**
 * Channel display names
 */
export const CHANNEL_NAMES: Record<ChannelType, string> = {
  whatsapp: 'WhatsApp',
  dingtalk: 'DingTalk',
  qqbot: 'QQ Bot',
  wecom: 'WeCom Bot',
  'wecom-app': 'WeCom App',
  telegram: 'Telegram',
  discord: 'Discord',
  signal: 'Signal',
  feishu: 'Feishu / Lark',
  imessage: 'iMessage',
  matrix: 'Matrix',
  line: 'LINE',
  msteams: 'Microsoft Teams',
  googlechat: 'Google Chat',
  mattermost: 'Mattermost',
};

/**
 * Channel metadata with configuration information
 */
export const CHANNEL_META: Record<ChannelType, ChannelMeta> = {
  qqbot: {
    id: 'qqbot',
    name: 'QQ Bot',
    icon: '🐧',
    description: 'channels:meta.qqbot.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.qqbot.docsUrl',
    configFields: [
      {
        key: 'appId',
        label: 'channels:meta.qqbot.fields.appId.label',
        type: 'text',
        placeholder: 'channels:meta.qqbot.fields.appId.placeholder',
        required: true,
      },
      {
        key: 'clientSecret',
        label: 'channels:meta.qqbot.fields.clientSecret.label',
        type: 'password',
        placeholder: 'channels:meta.qqbot.fields.clientSecret.placeholder',
        required: true,
      },
      {
        key: 'dmPolicy',
        label: 'channels:meta.qqbot.fields.dmPolicy.label',
        type: 'select',
        description: 'channels:meta.qqbot.fields.dmPolicy.description',
        defaultValue: 'open',
        options: [
          { value: 'open', label: 'channels:meta.qqbot.options.dmPolicy.open' },
          { value: 'pairing', label: 'channels:meta.qqbot.options.dmPolicy.pairing' },
          { value: 'allowlist', label: 'channels:meta.qqbot.options.dmPolicy.allowlist' },
        ],
      },
      {
        key: 'allowFrom',
        label: 'channels:meta.qqbot.fields.allowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.qqbot.fields.allowFrom.placeholder',
        description: 'channels:meta.qqbot.fields.allowFrom.description',
      },
      {
        key: 'groupPolicy',
        label: 'channels:meta.qqbot.fields.groupPolicy.label',
        type: 'select',
        description: 'channels:meta.qqbot.fields.groupPolicy.description',
        defaultValue: 'open',
        options: [
          { value: 'open', label: 'channels:meta.qqbot.options.groupPolicy.open' },
          { value: 'allowlist', label: 'channels:meta.qqbot.options.groupPolicy.allowlist' },
          { value: 'disabled', label: 'channels:meta.qqbot.options.groupPolicy.disabled' },
        ],
      },
      {
        key: 'groupAllowFrom',
        label: 'channels:meta.qqbot.fields.groupAllowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.qqbot.fields.groupAllowFrom.placeholder',
        description: 'channels:meta.qqbot.fields.groupAllowFrom.description',
      },
      {
        key: 'requireMention',
        label: 'channels:meta.qqbot.fields.requireMention.label',
        type: 'select',
        description: 'channels:meta.qqbot.fields.requireMention.description',
        defaultValue: 'true',
        options: [
          { value: 'true', label: 'channels:meta.qqbot.options.boolean.enabled' },
          { value: 'false', label: 'channels:meta.qqbot.options.boolean.disabled' },
        ],
      },
      {
        key: 'autoSendLocalPathMedia',
        label: 'channels:meta.qqbot.fields.autoSendLocalPathMedia.label',
        type: 'select',
        description: 'channels:meta.qqbot.fields.autoSendLocalPathMedia.description',
        defaultValue: 'true',
        options: [
          { value: 'true', label: 'channels:meta.qqbot.options.boolean.enabled' },
          { value: 'false', label: 'channels:meta.qqbot.options.boolean.disabled' },
        ],
      },
    ],
    instructions: [
      'channels:meta.qqbot.instructions.0',
      'channels:meta.qqbot.instructions.1',
      'channels:meta.qqbot.instructions.2',
      'channels:meta.qqbot.instructions.3',
    ],
    isPlugin: true,
  },
  wecom: {
    id: 'wecom',
    name: 'WeCom Bot',
    icon: '🏢',
    description: 'channels:meta.wecom.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.wecom.docsUrl',
    configFields: [
      {
        key: 'mode',
        label: 'channels:meta.wecom.fields.mode.label',
        type: 'select',
        description: 'channels:meta.wecom.fields.mode.description',
        defaultValue: 'ws',
        options: [
          { value: 'ws', label: 'channels:meta.wecom.options.mode.ws' },
          { value: 'webhook', label: 'channels:meta.wecom.options.mode.webhook' },
        ],
      },
      {
        key: 'botId',
        label: 'channels:meta.wecom.fields.botId.label',
        type: 'text',
        placeholder: 'channels:meta.wecom.fields.botId.placeholder',
      },
      {
        key: 'secret',
        label: 'channels:meta.wecom.fields.secret.label',
        type: 'password',
        placeholder: 'channels:meta.wecom.fields.secret.placeholder',
      },
      {
        key: 'webhookPath',
        label: 'channels:meta.wecom.fields.webhookPath.label',
        type: 'text',
        placeholder: 'channels:meta.wecom.fields.webhookPath.placeholder',
      },
      {
        key: 'token',
        label: 'channels:meta.wecom.fields.token.label',
        type: 'password',
        placeholder: 'channels:meta.wecom.fields.token.placeholder',
      },
      {
        key: 'encodingAESKey',
        label: 'channels:meta.wecom.fields.encodingAESKey.label',
        type: 'password',
        placeholder: 'channels:meta.wecom.fields.encodingAESKey.placeholder',
      },
      {
        key: 'publicBaseUrl',
        label: 'channels:meta.wecom.fields.publicBaseUrl.label',
        type: 'text',
        placeholder: 'channels:meta.wecom.fields.publicBaseUrl.placeholder',
      },
      {
        key: 'welcomeText',
        label: 'channels:meta.wecom.fields.welcomeText.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecom.fields.welcomeText.placeholder',
      },
      {
        key: 'dmPolicy',
        label: 'channels:meta.wecom.fields.dmPolicy.label',
        type: 'select',
        description: 'channels:meta.wecom.fields.dmPolicy.description',
        defaultValue: 'pairing',
        options: [
          { value: 'pairing', label: 'channels:meta.wecom.options.dmPolicy.pairing' },
          { value: 'allowlist', label: 'channels:meta.wecom.options.dmPolicy.allowlist' },
          { value: 'open', label: 'channels:meta.wecom.options.dmPolicy.open' },
          { value: 'disabled', label: 'channels:meta.wecom.options.dmPolicy.disabled' },
        ],
      },
      {
        key: 'allowFrom',
        label: 'channels:meta.wecom.fields.allowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecom.fields.allowFrom.placeholder',
        description: 'channels:meta.wecom.fields.allowFrom.description',
      },
      {
        key: 'groupPolicy',
        label: 'channels:meta.wecom.fields.groupPolicy.label',
        type: 'select',
        description: 'channels:meta.wecom.fields.groupPolicy.description',
        defaultValue: 'open',
        options: [
          { value: 'open', label: 'channels:meta.wecom.options.groupPolicy.open' },
          { value: 'allowlist', label: 'channels:meta.wecom.options.groupPolicy.allowlist' },
          { value: 'disabled', label: 'channels:meta.wecom.options.groupPolicy.disabled' },
        ],
      },
      {
        key: 'groupAllowFrom',
        label: 'channels:meta.wecom.fields.groupAllowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecom.fields.groupAllowFrom.placeholder',
        description: 'channels:meta.wecom.fields.groupAllowFrom.description',
      },
      {
        key: 'requireMention',
        label: 'channels:meta.wecom.fields.requireMention.label',
        type: 'select',
        description: 'channels:meta.wecom.fields.requireMention.description',
        defaultValue: 'true',
        options: [
          { value: 'true', label: 'channels:meta.wecom.options.boolean.enabled' },
          { value: 'false', label: 'channels:meta.wecom.options.boolean.disabled' },
        ],
      },
    ],
    instructions: [
      'channels:meta.wecom.instructions.0',
      'channels:meta.wecom.instructions.1',
      'channels:meta.wecom.instructions.2',
      'channels:meta.wecom.instructions.3',
    ],
    isPlugin: true,
  },
  'wecom-app': {
    id: 'wecom-app',
    name: 'WeCom App',
    icon: '🧩',
    description: 'channels:meta.wecomApp.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.wecomApp.docsUrl',
    configFields: [
      {
        key: 'corpId',
        label: 'channels:meta.wecomApp.fields.corpId.label',
        type: 'text',
        placeholder: 'channels:meta.wecomApp.fields.corpId.placeholder',
        required: true,
      },
      {
        key: 'corpSecret',
        label: 'channels:meta.wecomApp.fields.corpSecret.label',
        type: 'password',
        placeholder: 'channels:meta.wecomApp.fields.corpSecret.placeholder',
        required: true,
      },
      {
        key: 'agentId',
        label: 'channels:meta.wecomApp.fields.agentId.label',
        type: 'text',
        placeholder: 'channels:meta.wecomApp.fields.agentId.placeholder',
        required: true,
      },
      {
        key: 'webhookPath',
        label: 'channels:meta.wecomApp.fields.webhookPath.label',
        type: 'text',
        placeholder: 'channels:meta.wecomApp.fields.webhookPath.placeholder',
      },
      {
        key: 'token',
        label: 'channels:meta.wecomApp.fields.token.label',
        type: 'password',
        placeholder: 'channels:meta.wecomApp.fields.token.placeholder',
      },
      {
        key: 'encodingAESKey',
        label: 'channels:meta.wecomApp.fields.encodingAESKey.label',
        type: 'password',
        placeholder: 'channels:meta.wecomApp.fields.encodingAESKey.placeholder',
      },
      {
        key: 'apiBaseUrl',
        label: 'channels:meta.wecomApp.fields.apiBaseUrl.label',
        type: 'text',
        placeholder: 'channels:meta.wecomApp.fields.apiBaseUrl.placeholder',
      },
      {
        key: 'welcomeText',
        label: 'channels:meta.wecomApp.fields.welcomeText.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecomApp.fields.welcomeText.placeholder',
      },
      {
        key: 'dmPolicy',
        label: 'channels:meta.wecomApp.fields.dmPolicy.label',
        type: 'select',
        description: 'channels:meta.wecomApp.fields.dmPolicy.description',
        defaultValue: 'pairing',
        options: [
          { value: 'pairing', label: 'channels:meta.wecomApp.options.dmPolicy.pairing' },
          { value: 'allowlist', label: 'channels:meta.wecomApp.options.dmPolicy.allowlist' },
          { value: 'open', label: 'channels:meta.wecomApp.options.dmPolicy.open' },
          { value: 'disabled', label: 'channels:meta.wecomApp.options.dmPolicy.disabled' },
        ],
      },
      {
        key: 'allowFrom',
        label: 'channels:meta.wecomApp.fields.allowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecomApp.fields.allowFrom.placeholder',
        description: 'channels:meta.wecomApp.fields.allowFrom.description',
      },
      {
        key: 'groupPolicy',
        label: 'channels:meta.wecomApp.fields.groupPolicy.label',
        type: 'select',
        description: 'channels:meta.wecomApp.fields.groupPolicy.description',
        defaultValue: 'open',
        options: [
          { value: 'open', label: 'channels:meta.wecomApp.options.groupPolicy.open' },
          { value: 'allowlist', label: 'channels:meta.wecomApp.options.groupPolicy.allowlist' },
          { value: 'disabled', label: 'channels:meta.wecomApp.options.groupPolicy.disabled' },
        ],
      },
      {
        key: 'groupAllowFrom',
        label: 'channels:meta.wecomApp.fields.groupAllowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.wecomApp.fields.groupAllowFrom.placeholder',
        description: 'channels:meta.wecomApp.fields.groupAllowFrom.description',
      },
      {
        key: 'requireMention',
        label: 'channels:meta.wecomApp.fields.requireMention.label',
        type: 'select',
        description: 'channels:meta.wecomApp.fields.requireMention.description',
        defaultValue: 'true',
        options: [
          { value: 'true', label: 'channels:meta.wecomApp.options.boolean.enabled' },
          { value: 'false', label: 'channels:meta.wecomApp.options.boolean.disabled' },
        ],
      },
    ],
    instructions: [
      'channels:meta.wecomApp.instructions.0',
      'channels:meta.wecomApp.instructions.1',
      'channels:meta.wecomApp.instructions.2',
      'channels:meta.wecomApp.instructions.3',
    ],
    isPlugin: true,
  },
  dingtalk: {
    id: 'dingtalk',
    name: 'DingTalk',
    icon: '💬',
    description: 'channels:meta.dingtalk.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.dingtalk.docsUrl',
    configFields: [
      {
        key: 'clientId',
        label: 'channels:meta.dingtalk.fields.clientId.label',
        type: 'text',
        placeholder: 'channels:meta.dingtalk.fields.clientId.placeholder',
        required: true,
      },
      {
        key: 'clientSecret',
        label: 'channels:meta.dingtalk.fields.clientSecret.label',
        type: 'password',
        placeholder: 'channels:meta.dingtalk.fields.clientSecret.placeholder',
        required: true,
      },
      {
        key: 'robotCode',
        label: 'channels:meta.dingtalk.fields.robotCode.label',
        type: 'text',
        placeholder: 'channels:meta.dingtalk.fields.robotCode.placeholder',
        required: false,
      },
      {
        key: 'corpId',
        label: 'channels:meta.dingtalk.fields.corpId.label',
        type: 'text',
        placeholder: 'channels:meta.dingtalk.fields.corpId.placeholder',
        required: false,
      },
      {
        key: 'agentId',
        label: 'channels:meta.dingtalk.fields.agentId.label',
        type: 'text',
        placeholder: 'channels:meta.dingtalk.fields.agentId.placeholder',
        required: false,
      },
    ],
    instructions: [
      'channels:meta.dingtalk.instructions.0',
      'channels:meta.dingtalk.instructions.1',
      'channels:meta.dingtalk.instructions.2',
      'channels:meta.dingtalk.instructions.3',
    ],
    isPlugin: true,
  },
  telegram: {
    id: 'telegram',
    name: 'Telegram',
    icon: '✈️',
    description: 'channels:meta.telegram.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.telegram.docsUrl',
    configFields: [
      {
        key: 'botToken',
        label: 'channels:meta.telegram.fields.botToken.label',
        type: 'password',
        placeholder: 'channels:meta.telegram.fields.botToken.placeholder',
        required: true,
        envVar: 'TELEGRAM_BOT_TOKEN',
      },
      {
        key: 'allowedUsers',
        label: 'channels:meta.telegram.fields.allowedUsers.label',
        type: 'text',
        placeholder: 'channels:meta.telegram.fields.allowedUsers.placeholder',
        description: 'channels:meta.telegram.fields.allowedUsers.description',
        required: true,
      },
    ],
    instructions: [
      'channels:meta.telegram.instructions.0',
      'channels:meta.telegram.instructions.1',
      'channels:meta.telegram.instructions.2',
      'channels:meta.telegram.instructions.3',
      'channels:meta.telegram.instructions.4',
    ],
  },
  discord: {
    id: 'discord',
    name: 'Discord',
    icon: '🎮',
    description: 'channels:meta.discord.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.discord.docsUrl',
    configFields: [
      {
        key: 'token',
        label: 'channels:meta.discord.fields.token.label',
        type: 'password',
        placeholder: 'channels:meta.discord.fields.token.placeholder',
        required: true,
        envVar: 'DISCORD_BOT_TOKEN',
      },
      {
        key: 'guildId',
        label: 'channels:meta.discord.fields.guildId.label',
        type: 'text',
        placeholder: 'channels:meta.discord.fields.guildId.placeholder',
        required: true,
        description: 'channels:meta.discord.fields.guildId.description',
      },
      {
        key: 'channelId',
        label: 'channels:meta.discord.fields.channelId.label',
        type: 'text',
        placeholder: 'channels:meta.discord.fields.channelId.placeholder',
        required: false,
        description: 'channels:meta.discord.fields.channelId.description',
      },
    ],
    instructions: [
      'channels:meta.discord.instructions.0',
      'channels:meta.discord.instructions.1',
      'channels:meta.discord.instructions.2',
      'channels:meta.discord.instructions.3',
      'channels:meta.discord.instructions.4',
      'channels:meta.discord.instructions.5',
    ],
  },

  whatsapp: {
    id: 'whatsapp',
    name: 'WhatsApp',
    icon: '📱',
    description: 'channels:meta.whatsapp.description',
    connectionType: 'qr',
    docsUrl: 'channels:meta.whatsapp.docsUrl',
    configFields: [],
    instructions: [
      'channels:meta.whatsapp.instructions.0',
      'channels:meta.whatsapp.instructions.1',
      'channels:meta.whatsapp.instructions.2',
      'channels:meta.whatsapp.instructions.3',
    ],
  },
  signal: {
    id: 'signal',
    name: 'Signal',
    icon: '🔒',
    description: 'channels:meta.signal.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.signal.docsUrl',
    configFields: [
      {
        key: 'phoneNumber',
        label: 'channels:meta.signal.fields.phoneNumber.label',
        type: 'text',
        placeholder: 'channels:meta.signal.fields.phoneNumber.placeholder',
        required: true,
      },
    ],
    instructions: [
      'channels:meta.signal.instructions.0',
      'channels:meta.signal.instructions.1',
      'channels:meta.signal.instructions.2',
    ],
  },
  feishu: {
    id: 'feishu',
    name: 'Feishu / Lark',
    icon: '🐦',
    description: 'channels:meta.feishu.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.feishu.docsUrl',
    configFields: [
      {
        key: 'appId',
        label: 'channels:meta.feishu.fields.appId.label',
        type: 'text',
        placeholder: 'channels:meta.feishu.fields.appId.placeholder',
        required: true,
        envVar: 'FEISHU_APP_ID',
      },
      {
        key: 'appSecret',
        label: 'channels:meta.feishu.fields.appSecret.label',
        type: 'password',
        placeholder: 'channels:meta.feishu.fields.appSecret.placeholder',
        required: true,
        envVar: 'FEISHU_APP_SECRET',
      },
    ],
    instructions: [
      'channels:meta.feishu.instructions.0',
      'channels:meta.feishu.instructions.1',
      'channels:meta.feishu.instructions.2',
      'channels:meta.feishu.instructions.3',
      'channels:meta.feishu.instructions.4',
    ],
    isPlugin: true,
  },
  imessage: {
    id: 'imessage',
    name: 'iMessage',
    icon: '💬',
    description: 'channels:meta.imessage.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.imessage.docsUrl',
    configFields: [
      {
        key: 'serverUrl',
        label: 'channels:meta.imessage.fields.serverUrl.label',
        type: 'text',
        placeholder: 'channels:meta.imessage.fields.serverUrl.placeholder',
        required: true,
      },
      {
        key: 'password',
        label: 'channels:meta.imessage.fields.password.label',
        type: 'password',
        placeholder: 'channels:meta.imessage.fields.password.placeholder',
        required: true,
      },
    ],
    instructions: [
      'channels:meta.imessage.instructions.0',
      'channels:meta.imessage.instructions.1',
      'channels:meta.imessage.instructions.2',
    ],
  },
  matrix: {
    id: 'matrix',
    name: 'Matrix',
    icon: '🔗',
    description: 'channels:meta.matrix.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.matrix.docsUrl',
    configFields: [
      {
        key: 'homeserver',
        label: 'channels:meta.matrix.fields.homeserver.label',
        type: 'text',
        placeholder: 'channels:meta.matrix.fields.homeserver.placeholder',
        required: true,
      },
      {
        key: 'accessToken',
        label: 'channels:meta.matrix.fields.accessToken.label',
        type: 'password',
        placeholder: 'channels:meta.matrix.fields.accessToken.placeholder',
        required: true,
      },
      {
        key: 'encryption',
        label: 'channels:meta.matrix.fields.encryption.label',
        type: 'select',
        description: 'channels:meta.matrix.fields.encryption.description',
        defaultValue: 'false',
        options: [
          {
            value: 'false',
            label: 'channels:meta.matrix.options.boolean.disabled',
          },
          {
            value: 'true',
            label: 'channels:meta.matrix.options.boolean.enabled',
          },
        ],
      },
      {
        key: 'dmPolicy',
        label: 'channels:meta.matrix.fields.dmPolicy.label',
        type: 'select',
        description: 'channels:meta.matrix.fields.dmPolicy.description',
        defaultValue: 'pairing',
        options: [
          {
            value: 'pairing',
            label: 'channels:meta.matrix.options.dmPolicy.pairing',
          },
          {
            value: 'allowlist',
            label: 'channels:meta.matrix.options.dmPolicy.allowlist',
          },
          {
            value: 'open',
            label: 'channels:meta.matrix.options.dmPolicy.open',
          },
          {
            value: 'disabled',
            label: 'channels:meta.matrix.options.dmPolicy.disabled',
          },
        ],
      },
      {
        key: 'dmAllowFrom',
        label: 'channels:meta.matrix.fields.dmAllowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.matrix.fields.dmAllowFrom.placeholder',
        description: 'channels:meta.matrix.fields.dmAllowFrom.description',
      },
      {
        key: 'groupPolicy',
        label: 'channels:meta.matrix.fields.groupPolicy.label',
        type: 'select',
        description: 'channels:meta.matrix.fields.groupPolicy.description',
        defaultValue: 'allowlist',
        options: [
          {
            value: 'allowlist',
            label: 'channels:meta.matrix.options.groupPolicy.allowlist',
          },
          {
            value: 'open',
            label: 'channels:meta.matrix.options.groupPolicy.open',
          },
          {
            value: 'disabled',
            label: 'channels:meta.matrix.options.groupPolicy.disabled',
          },
        ],
      },
      {
        key: 'groups',
        label: 'channels:meta.matrix.fields.groups.label',
        type: 'textarea',
        placeholder: 'channels:meta.matrix.fields.groups.placeholder',
        description: 'channels:meta.matrix.fields.groups.description',
      },
      {
        key: 'groupAllowFrom',
        label: 'channels:meta.matrix.fields.groupAllowFrom.label',
        type: 'textarea',
        placeholder: 'channels:meta.matrix.fields.groupAllowFrom.placeholder',
        description: 'channels:meta.matrix.fields.groupAllowFrom.description',
      },
      {
        key: 'roomReplyMode',
        label: 'channels:meta.matrix.fields.roomReplyMode.label',
        type: 'select',
        description: 'channels:meta.matrix.fields.roomReplyMode.description',
        defaultValue: 'mentionOnly',
        options: [
          {
            value: 'mentionOnly',
            label: 'channels:meta.matrix.options.roomReplyMode.mentionOnly',
          },
          {
            value: 'autoReply',
            label: 'channels:meta.matrix.options.roomReplyMode.autoReply',
          },
        ],
      },
    ],
    instructions: [
      'channels:meta.matrix.instructions.0',
      'channels:meta.matrix.instructions.1',
      'channels:meta.matrix.instructions.2',
    ],
    isPlugin: true,
  },
  line: {
    id: 'line',
    name: 'LINE',
    icon: '🟢',
    description: 'channels:meta.line.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.line.docsUrl',
    configFields: [
      {
        key: 'channelAccessToken',
        label: 'channels:meta.line.fields.channelAccessToken.label',
        type: 'password',
        placeholder: 'channels:meta.line.fields.channelAccessToken.placeholder',
        required: true,
        envVar: 'LINE_CHANNEL_ACCESS_TOKEN',
      },
      {
        key: 'channelSecret',
        label: 'channels:meta.line.fields.channelSecret.label',
        type: 'password',
        placeholder: 'channels:meta.line.fields.channelSecret.placeholder',
        required: true,
        envVar: 'LINE_CHANNEL_SECRET',
      },
    ],
    instructions: [
      'channels:meta.line.instructions.0',
      'channels:meta.line.instructions.1',
      'channels:meta.line.instructions.2',
    ],
    isPlugin: true,
  },
  msteams: {
    id: 'msteams',
    name: 'Microsoft Teams',
    icon: '👔',
    description: 'channels:meta.msteams.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.msteams.docsUrl',
    configFields: [
      {
        key: 'appId',
        label: 'channels:meta.msteams.fields.appId.label',
        type: 'text',
        placeholder: 'channels:meta.msteams.fields.appId.placeholder',
        required: true,
        envVar: 'MSTEAMS_APP_ID',
      },
      {
        key: 'appPassword',
        label: 'channels:meta.msteams.fields.appPassword.label',
        type: 'password',
        placeholder: 'channels:meta.msteams.fields.appPassword.placeholder',
        required: true,
        envVar: 'MSTEAMS_APP_PASSWORD',
      },
    ],
    instructions: [
      'channels:meta.msteams.instructions.0',
      'channels:meta.msteams.instructions.1',
      'channels:meta.msteams.instructions.2',
      'channels:meta.msteams.instructions.3',
    ],
    isPlugin: true,
  },
  googlechat: {
    id: 'googlechat',
    name: 'Google Chat',
    icon: '💭',
    description: 'channels:meta.googlechat.description',
    connectionType: 'webhook',
    docsUrl: 'channels:meta.googlechat.docsUrl',
    configFields: [
      {
        key: 'serviceAccountKey',
        label: 'channels:meta.googlechat.fields.serviceAccountKey.label',
        type: 'text',
        placeholder: 'channels:meta.googlechat.fields.serviceAccountKey.placeholder',
        required: true,
      },
    ],
    instructions: [
      'channels:meta.googlechat.instructions.0',
      'channels:meta.googlechat.instructions.1',
      'channels:meta.googlechat.instructions.2',
      'channels:meta.googlechat.instructions.3',
    ],
  },
  mattermost: {
    id: 'mattermost',
    name: 'Mattermost',
    icon: '💠',
    description: 'channels:meta.mattermost.description',
    connectionType: 'token',
    docsUrl: 'channels:meta.mattermost.docsUrl',
    configFields: [
      {
        key: 'serverUrl',
        label: 'channels:meta.mattermost.fields.serverUrl.label',
        type: 'text',
        placeholder: 'channels:meta.mattermost.fields.serverUrl.placeholder',
        required: true,
      },
      {
        key: 'botToken',
        label: 'channels:meta.mattermost.fields.botToken.label',
        type: 'password',
        placeholder: 'channels:meta.mattermost.fields.botToken.placeholder',
        required: true,
      },
    ],
    instructions: [
      'channels:meta.mattermost.instructions.0',
      'channels:meta.mattermost.instructions.1',
      'channels:meta.mattermost.instructions.2',
    ],
    isPlugin: true,
  },
};

/**
 * Get primary supported channels (non-plugin, commonly used)
 */
export function getPrimaryChannels(): ChannelType[] {
  return [
    'telegram',
    'discord',
    'whatsapp',
    'qqbot',
    'dingtalk',
    'wecom',
    'wecom-app',
    'feishu',
    'matrix',
  ];
}

export function getDefaultChannelConfigValues(type: ChannelType): Record<string, string> {
  return CHANNEL_META[type].configFields.reduce<Record<string, string>>((defaults, field) => {
    if (typeof field.defaultValue === 'string') {
      defaults[field.key] = field.defaultValue;
    }
    return defaults;
  }, {});
}

/**
 * Get all available channels including plugins
 */
export function getAllChannels(): ChannelType[] {
  return Object.keys(CHANNEL_META) as ChannelType[];
}
