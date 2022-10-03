const polkadotBabelConfig = require('@polkadot/dev/config/babel-config-cjs.cjs');

module.exports = {
  plugins: [
    ...polkadotBabelConfig.plugins,
  ],
  presets: [
    ...polkadotBabelConfig.presets
  ]
};
