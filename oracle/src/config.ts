export interface OracleConfig {
  maxRetries: number;
  backoffBaseMs: number;
  backoffCapMs: number;
}

export const config: OracleConfig = {
  maxRetries: 3,
  backoffBaseMs: 1000, 
  backoffCapMs: 10000, 
};
